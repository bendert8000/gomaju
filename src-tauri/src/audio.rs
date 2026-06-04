use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use rodio::source::{SineWave, Source};
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};
use tauri::{AppHandle, Emitter};

use restee_core::chime::{is_safe_filename, ChimeDto, ChimeKindDto, ToneStep};

/// Play a sound on a detached thread: acquire the default output + a sink, let `fill`
/// enqueue its tones, then block that thread until playback ends. Audio is best-effort —
/// a missing device or sink failure is logged and ignored (sound is never essential).
/// `what` names the sound for the completion log line.
fn play<F>(what: &'static str, fill: F)
where
    F: FnOnce(&Sink) + Send + 'static,
{
    std::thread::spawn(move || {
        let (_stream, handle) = match OutputStream::try_default() {
            Ok(out) => out,
            Err(e) => {
                eprintln!("restee: no audio output device ({e})");
                return;
            }
        };
        let sink = match Sink::try_new(&handle) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("restee: could not create audio sink ({e})");
                return;
            }
        };

        fill(&sink);
        sink.append(silence(SYNTH_TAIL_MS)); // drain on silence so the stream drop doesn't pop

        // Keep `_stream` alive until playback finishes.
        sink.sleep_until_end();
        eprintln!("restee: {what} played");
    });
}

/// Play a gentle two-note chime. Generated in code (no bundled asset) at low amplitude
/// with a short fade-in, so it reads as a soft cue rather than a harsh beep.
pub fn play_chime() {
    play("chime", |sink| {
        let note = |freq: f32, ms: u64| {
            SineWave::new(freq)
                .take_duration(Duration::from_millis(ms))
                .amplify(0.18)
                .fade_in(Duration::from_millis(40))
        };
        sink.append(note(659.25, 200)); // E5
        sink.append(note(987.77, 320)); // B5
    });
}

/// Play a short, gentle "break over" cue: a brief descending two-note (the inverse of the
/// rising start chime) so it reads as "done — resume". Same low amplitude as the chime.
pub fn play_break_over() {
    play("break-over chime", |sink| {
        let note = |freq: f32, ms: u64| {
            SineWave::new(freq)
                .take_duration(Duration::from_millis(ms))
                .amplify(0.18)
                .fade_in(Duration::from_millis(30))
        };
        sink.append(note(987.77, 140)); // B5
        sink.append(note(659.25, 200)); // E5 (descending -> resolved)
    });
}

/// Play a distinct, attention-grabbing alarm tone: louder than the chime (0.35 vs 0.18),
/// a two-tone beep repeated a few times — but strictly bounded (~3.5s, no infinite loop),
/// since a runaway alarm in a break app is worse than a missed one. Used by the alarm
/// scheduler, independent of the break `sound` setting.
pub fn play_alarm() {
    play("alarm tone", |sink| {
        let beep = |freq: f32, ms: u64| {
            SineWave::new(freq)
                .take_duration(Duration::from_millis(ms))
                .amplify(0.35)
                .fade_in(Duration::from_millis(15))
        };
        // Five high/low cycles ~= 3.5s total, each followed by a short silent gap.
        for _ in 0..5 {
            sink.append(beep(880.0, 250)); // A5
            sink.append(beep(1174.66, 250)); // D6
            sink.append(
                SineWave::new(0.0)
                    .take_duration(Duration::from_millis(200))
                    .amplify(0.0),
            );
        }
    });
}

/// Synthesis sample rate for tone steps. rodio resamples to the device rate as needed.
const SYNTH_SAMPLE_RATE: u32 = 44_100;
/// Short release (fade-out to zero) at the end of every tone step. Without it, a sine cut off
/// mid-cycle at non-zero amplitude makes an audible click/pop ("explosion") between notes; ramping
/// to zero over the last few ms removes the discontinuity. Clamped to never exceed the note.
const SYNTH_RELEASE_MS: u32 = 8;

/// Synthesize one tone step as a buffered sine with an attack (fade-in over `fade_in_ms`) and a
/// short release (fade-out over `SYNTH_RELEASE_MS`) envelope, so notes start and **end** at zero
/// amplitude and never click. `freq_hz == 0` yields silence (a rest/gap).
fn tone_source(step: &ToneStep) -> SamplesBuffer<f32> {
    let total =
        (SYNTH_SAMPLE_RATE as u64 * step.duration_ms.max(1) as u64 / 1000).max(1) as usize;
    let attack = (SYNTH_SAMPLE_RATE as u64 * step.fade_in_ms as u64 / 1000) as usize;
    // Keep the release within the note (and leave room for the attack on very short notes).
    let release =
        ((SYNTH_SAMPLE_RATE as u64 * SYNTH_RELEASE_MS as u64 / 1000) as usize).min(total / 2);
    let mut samples = Vec::with_capacity(total);
    for i in 0..total {
        // Full-scale sine; the whole-chime volume is applied via the sink (`set_volume`). A
        // `freq_hz == 0` step is `sin(0) == 0` -> silence.
        let mut s =
            (std::f32::consts::TAU * step.freq_hz as f32 * (i as f32 / SYNTH_SAMPLE_RATE as f32)).sin();
        if attack > 0 && i < attack {
            s *= i as f32 / attack as f32; // attack (fade in)
        }
        let from_end = total - 1 - i;
        if release > 0 && from_end < release {
            s *= from_end as f32 / release as f32; // release (fade out) -> 0 on the last sample
        }
        samples.push(s);
    }
    SamplesBuffer::new(1, SYNTH_SAMPLE_RATE, samples)
}

/// Trailing silence appended after every sound. Dropping the output stream stops the device
/// *mid-buffer*, so without this the device's still-buffered tail (~10–30 ms of output latency) is
/// truncated at close, making a faint pop at the very end. Padding with silence lets that buffer
/// drain to zero before the stream drops, so the close is silent.
const SYNTH_TAIL_MS: u32 = 80;

/// A buffer of `ms` of silence (for the trailing pad before stream teardown).
fn silence(ms: u32) -> SamplesBuffer<f32> {
    let n = (SYNTH_SAMPLE_RATE as u64 * ms as u64 / 1000).max(1) as usize;
    SamplesBuffer::new(1, SYNTH_SAMPLE_RATE, vec![0.0f32; n])
}

/// Play a user-defined synthesized chime: a sequence of tone steps at the chime's `volume_pct`
/// (applied to the whole sink). A `freq_hz == 0` step is silence (a gap); each step gets an attack
/// + short release so notes don't click (`tone_source`).
pub fn play_chime_spec(steps: Vec<ToneStep>, volume_pct: u8) {
    play("custom chime", move |sink| {
        sink.set_volume(volume_pct as f32 / 100.0);
        for s in &steps {
            sink.append(tone_source(s));
        }
    });
}

/// Play an imported audio-file chime (wav/mp3/ogg/flac via rodio's default decoders) at the chime's
/// `volume_pct` (a percent of the file's native level). Best-effort: a missing or undecodable file
/// is logged and ignored.
pub fn play_chime_file(path: PathBuf, volume_pct: u8) {
    play("file chime", move |sink| {
        sink.set_volume(volume_pct as f32 / 100.0);
        match std::fs::File::open(&path) {
            Ok(file) => match rodio::Decoder::new(std::io::BufReader::new(file)) {
                Ok(source) => sink.append(source),
                Err(e) => eprintln!("restee: could not decode chime {} ({e})", path.display()),
            },
            Err(e) => eprintln!("restee: could not open chime {} ({e})", path.display()),
        }
    });
}

/// Play the chime referenced by `chime_id` (looked up in `chimes`), falling back to `default`
/// when it's unassigned, missing, or malformed. `chimes_dir` is where imported files live; file
/// names are re-checked with `is_safe_filename` and joined only under it (never a raw path).
fn play_assigned_or(chime_id: &str, chimes: &[ChimeDto], chimes_dir: &Path, default: fn()) {
    if !chime_id.is_empty() {
        if let Some(c) = chimes.iter().find(|c| c.id == chime_id) {
            match c.kind {
                ChimeKindDto::Tones if !c.steps.is_empty() => {
                    play_chime_spec(c.steps.clone(), c.volume_pct);
                    return;
                }
                ChimeKindDto::File if is_safe_filename(&c.file) => {
                    play_chime_file(chimes_dir.join(&c.file), c.volume_pct);
                    return;
                }
                _ => {} // malformed -> fall through to the default tone
            }
        }
    }
    default();
}

/// Break-start cue: the rule's assigned chime, or the built-in default chime.
pub fn play_break_chime(chime_id: &str, chimes: &[ChimeDto], chimes_dir: &Path) {
    play_assigned_or(chime_id, chimes, chimes_dir, play_chime);
}

/// Break-end cue: the rule's assigned end chime, or the built-in default break-over tone.
pub fn play_break_over_chime(chime_id: &str, chimes: &[ChimeDto], chimes_dir: &Path) {
    play_assigned_or(chime_id, chimes, chimes_dir, play_break_over);
}

/// Alarm cue: the alarm's assigned chime, or the built-in default alarm tone.
pub fn play_alarm_chime(chime_id: &str, chimes: &[ChimeDto], chimes_dir: &Path) {
    play_assigned_or(chime_id, chimes, chimes_dir, play_alarm);
}

// --- Stoppable preview (Chimes window) ---
//
// The break/alarm cues above are fire-and-forget. The Chimes-window Preview button needs to *stop* a
// playing chime and learn when it ends, so it can toggle ▶ Preview ⇄ ⏸ Pause. We track a single
// current preview behind a generation token: starting a new preview (or stopping) bumps the gen and
// stops the old sink; when a preview thread finishes it emits `preview-ended` carrying its gen — but
// only if it's still the current one, so a superseded preview never reverts the wrong button.

#[derive(Default)]
struct PreviewState {
    gen: u64,
    sink: Option<Arc<Sink>>,
}

static PREVIEW: LazyLock<Mutex<PreviewState>> = LazyLock::new(|| Mutex::new(PreviewState::default()));

/// Stop the current preview (if any). Bumps the gen so the playing thread won't emit `preview-ended`
/// (the caller already knows it stopped). No-op when nothing is playing.
pub fn stop_preview() {
    let mut p = PREVIEW.lock().unwrap();
    p.gen += 1;
    if let Some(sink) = p.sink.take() {
        sink.stop();
    }
}

/// Start a stoppable preview on a detached thread, superseding any current one. Returns the
/// generation token the frontend matches against the `preview-ended` event. The gen starts at 1, so
/// a returned value is always truthy ("playing"); the command returns 0 only when nothing plays.
fn start_preview<F>(app: AppHandle, fill: F) -> u64
where
    F: FnOnce(&Sink) + Send + 'static,
{
    let gen = {
        let mut p = PREVIEW.lock().unwrap();
        p.gen += 1;
        if let Some(sink) = p.sink.take() {
            sink.stop(); // stop whatever was playing
        }
        p.gen
    };
    std::thread::spawn(move || {
        let (_stream, handle) = match OutputStream::try_default() {
            Ok(out) => out,
            Err(e) => {
                eprintln!("restee: no audio output device ({e})");
                finish_preview(&app, gen);
                return;
            }
        };
        let sink = match Sink::try_new(&handle) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                eprintln!("restee: could not create audio sink ({e})");
                finish_preview(&app, gen);
                return;
            }
        };
        fill(&sink);
        sink.append(silence(SYNTH_TAIL_MS)); // drain on silence so the stream drop doesn't pop
        // Register as the current preview — unless a newer preview/stop superseded us while we set up.
        {
            let mut p = PREVIEW.lock().unwrap();
            if p.gen != gen {
                return; // superseded: the newer preview owns the lifecycle; don't play or notify
            }
            p.sink = Some(Arc::clone(&sink));
        }
        sink.sleep_until_end(); // returns on natural end or when stopped
        finish_preview(&app, gen);
    });
    gen
}

/// On a preview thread's exit: if it's still the current generation, clear it and tell the UI the
/// preview ended (so the button reverts to ▶). A superseded preview does nothing.
fn finish_preview(app: &AppHandle, gen: u64) {
    let mut p = PREVIEW.lock().unwrap();
    if p.gen == gen {
        p.sink = None;
        let _ = app.emit("preview-ended", gen);
    }
}

/// Preview a synthesized tone sequence at `volume_pct` (stoppable); matches `play_chime_spec`.
pub fn preview_chime_spec(app: AppHandle, steps: Vec<ToneStep>, volume_pct: u8) -> u64 {
    start_preview(app, move |sink| {
        sink.set_volume(volume_pct as f32 / 100.0);
        for s in &steps {
            sink.append(tone_source(s));
        }
    })
}

/// Preview an imported audio file at `volume_pct` (stoppable); matches `play_chime_file`.
pub fn preview_chime_file(app: AppHandle, path: PathBuf, volume_pct: u8) -> u64 {
    start_preview(app, move |sink| {
        sink.set_volume(volume_pct as f32 / 100.0);
        match std::fs::File::open(&path) {
            Ok(file) => match rodio::Decoder::new(std::io::BufReader::new(file)) {
                Ok(source) => sink.append(source),
                Err(e) => eprintln!("restee: could not decode chime {} ({e})", path.display()),
            },
            Err(e) => eprintln!("restee: could not open chime {} ({e})", path.display()),
        }
    })
}
