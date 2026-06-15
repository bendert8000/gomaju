use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use rodio::source::{SineWave, Source};
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};
use tauri::{AppHandle, Emitter};

use gomaju_core::chime::{is_safe_filename, ChimeDto, ChimeKindDto, ToneStep};

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
                crate::rlog!("gomaju: no audio output device ({e})");
                return;
            }
        };
        let sink = match Sink::try_new(&handle) {
            Ok(s) => s,
            Err(e) => {
                crate::rlog!("gomaju: could not create audio sink ({e})");
                return;
            }
        };

        fill(&sink);
        sink.append(silence(SYNTH_TAIL_MS)); // drain on silence so the stream drop doesn't pop

        // Keep `_stream` alive until playback finishes.
        sink.sleep_until_end();
        crate::rlog!("gomaju: {what} played");
    });
}

/// Fill for the break-start chime. A named fn (not an inline closure) so it can be reused as the
/// break-start default-tone *preview* (`preview_default`), not just the fire-and-forget cue.
fn fill_break_start(sink: &Sink) {
    let note = |freq: f32, ms: u64| {
        SineWave::new(freq)
            .take_duration(Duration::from_millis(ms))
            .amplify(0.18)
            .fade_in(Duration::from_millis(40))
    };
    sink.append(note(659.25, 200)); // E5
    sink.append(note(987.77, 320)); // B5
}

/// Fill for the break-over cue (reused as the break-over default-tone preview).
fn fill_break_over(sink: &Sink) {
    let note = |freq: f32, ms: u64| {
        SineWave::new(freq)
            .take_duration(Duration::from_millis(ms))
            .amplify(0.18)
            .fade_in(Duration::from_millis(30))
    };
    sink.append(note(987.77, 140)); // B5
    sink.append(note(659.25, 200)); // E5 (descending -> resolved)
}

/// Fill for the alarm tone (reused as the alarm default-tone preview). Bounded (~3.5s), so a preview
/// of it ends on its own just like the fire-and-forget alarm cue.
fn fill_alarm(sink: &Sink) {
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
    let total = (SYNTH_SAMPLE_RATE as u64 * step.duration_ms.max(1) as u64 / 1000).max(1) as usize;
    let attack = (SYNTH_SAMPLE_RATE as u64 * step.fade_in_ms as u64 / 1000) as usize;
    // Keep the release within the note (and leave room for the attack on very short notes).
    let release =
        ((SYNTH_SAMPLE_RATE as u64 * SYNTH_RELEASE_MS as u64 / 1000) as usize).min(total / 2);
    let mut samples = Vec::with_capacity(total);
    for i in 0..total {
        // Full-scale sine; the caller's assignment volume is applied via the sink (`set_volume`). A
        // `freq_hz == 0` step is `sin(0) == 0` -> silence.
        let mut s =
            (std::f32::consts::TAU * step.freq_hz as f32 * (i as f32 / SYNTH_SAMPLE_RATE as f32))
                .sin();
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

/// Play a user-defined synthesized chime: a sequence of tone steps at the caller's `volume_pct`
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

/// Play an imported audio-file chime (wav/mp3/ogg/flac via rodio's default decoders) at the caller's
/// `volume_pct` (a percent of the file's native level). Best-effort: a missing or undecodable file
/// is logged and ignored.
pub fn play_chime_file(path: PathBuf, volume_pct: u8) {
    play("file chime", move |sink| {
        sink.set_volume(volume_pct as f32 / 100.0);
        match std::fs::File::open(&path) {
            Ok(file) => match rodio::Decoder::new(std::io::BufReader::new(file)) {
                Ok(source) => sink.append(source),
                Err(e) => crate::rlog!("gomaju: could not decode chime {} ({e})", path.display()),
            },
            Err(e) => crate::rlog!("gomaju: could not open chime {} ({e})", path.display()),
        }
    });
}

fn play_default_tone(what: &'static str, fill: fn(&Sink), volume_pct: u8) {
    play(what, move |sink| {
        sink.set_volume(volume_pct as f32 / 100.0);
        fill(sink);
    });
}

/// Reserved chime-picker value meaning "play no sound" — distinct from an empty id, which means the
/// built-in default tone. Must stay in sync with `NONE_CHIME` in `src/rule-editor.ts`.
pub const NONE_CHIME_ID: &str = "__none__";

/// Play the chime referenced by `chime_id` (looked up in `chimes`) at `volume_pct`, falling back to
/// the given built-in tone when it's unassigned, missing, or malformed. `chimes_dir` is where
/// imported files live; file names are re-checked with `is_safe_filename` and joined only under it.
/// `NONE_CHIME_ID` plays nothing.
fn play_assigned_or(
    chime_id: &str,
    volume_pct: u8,
    chimes: &[ChimeDto],
    chimes_dir: &Path,
    default_what: &'static str,
    default: fn(&Sink),
) {
    if chime_id == NONE_CHIME_ID {
        return; // explicit "None" -> silence
    }
    if !chime_id.is_empty() {
        if let Some(c) = chimes.iter().find(|c| c.id == chime_id) {
            match c.kind {
                ChimeKindDto::Tones if !c.steps.is_empty() => {
                    play_chime_spec(c.steps.clone(), volume_pct);
                    return;
                }
                ChimeKindDto::File if is_safe_filename(&c.file) => {
                    play_chime_file(chimes_dir.join(&c.file), volume_pct);
                    return;
                }
                _ => {} // malformed -> fall through to the default tone
            }
        }
    }
    play_default_tone(default_what, default, volume_pct);
}

/// Break-start cue: the rule's assigned chime, or the built-in default chime.
pub fn play_break_chime(chime_id: &str, volume_pct: u8, chimes: &[ChimeDto], chimes_dir: &Path) {
    play_assigned_or(
        chime_id,
        volume_pct,
        chimes,
        chimes_dir,
        "chime",
        fill_break_start,
    );
}

/// Break-end cue: the rule's assigned end chime, or the built-in default break-over tone.
pub fn play_break_over_chime(
    chime_id: &str,
    volume_pct: u8,
    chimes: &[ChimeDto],
    chimes_dir: &Path,
) {
    play_assigned_or(
        chime_id,
        volume_pct,
        chimes,
        chimes_dir,
        "break-over chime",
        fill_break_over,
    );
}

/// Alarm cue: the alarm's assigned chime, or the built-in default alarm tone.
pub fn play_alarm_chime(chime_id: &str, volume_pct: u8, chimes: &[ChimeDto], chimes_dir: &Path) {
    play_assigned_or(
        chime_id,
        volume_pct,
        chimes,
        chimes_dir,
        "alarm tone",
        fill_alarm,
    );
}

/// True while a countdown chime is sounding. Several countdown timers can come due in the same
/// tick; the cues are fire-and-forget, so without a guard they'd stack unbounded overlapping
/// audio threads. We allow at most one countdown chime at a time:
/// if one is already sounding, additional fires are skipped here (the scheduler still shows
/// the notification). This is the countdown equivalent of the alarm "one chime per minute" rule.
static COUNTDOWN_SOUNDING: AtomicBool = AtomicBool::new(false);

/// Like [`play`], but single-slot for countdowns: if a countdown chime is already sounding, skip.
/// A drop guard clears the busy flag however the thread exits (device error, decode error, or
/// natural end), so a one-off failure can never wedge the slot shut.
fn play_countdown<F>(fill: F)
where
    F: FnOnce(&Sink) + Send + 'static,
{
    if COUNTDOWN_SOUNDING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return; // already sounding -> coalesce this fire
    }
    std::thread::spawn(move || {
        struct Busy;
        impl Drop for Busy {
            fn drop(&mut self) {
                COUNTDOWN_SOUNDING.store(false, Ordering::Release);
            }
        }
        let _busy = Busy;

        let (_stream, handle) = match OutputStream::try_default() {
            Ok(out) => out,
            Err(e) => {
                crate::rlog!("gomaju: no audio output device ({e})");
                return;
            }
        };
        let sink = match Sink::try_new(&handle) {
            Ok(s) => s,
            Err(e) => {
                crate::rlog!("gomaju: could not create audio sink ({e})");
                return;
            }
        };
        fill(&sink);
        sink.append(silence(SYNTH_TAIL_MS)); // drain on silence so the stream drop doesn't pop
        sink.sleep_until_end();
        crate::rlog!("gomaju: countdown chime played");
    });
}

/// Countdown cue: the timer's assigned chime, or the built-in default tone (the alarm tone,
/// reused so there's no new sound to maintain). Single-slot via [`play_countdown`] so a burst of
/// simultaneous timers can't pile up overlapping audio. Resolution mirrors [`play_assigned_or`];
/// `NONE_CHIME_ID` plays nothing.
pub fn play_countdown_chime(chime_id: &str, volume_pct: u8, chimes: &[ChimeDto], chimes_dir: &Path) {
    if chime_id == NONE_CHIME_ID {
        return; // explicit "None" -> silence
    }
    if !chime_id.is_empty() {
        if let Some(c) = chimes.iter().find(|c| c.id == chime_id) {
            match c.kind {
                ChimeKindDto::Tones if !c.steps.is_empty() => {
                    let steps = c.steps.clone();
                    play_countdown(move |sink| {
                        sink.set_volume(volume_pct as f32 / 100.0);
                        for s in &steps {
                            sink.append(tone_source(s));
                        }
                    });
                    return;
                }
                ChimeKindDto::File if is_safe_filename(&c.file) => {
                    let path = chimes_dir.join(&c.file);
                    play_countdown(move |sink| {
                        sink.set_volume(volume_pct as f32 / 100.0);
                        match std::fs::File::open(&path) {
                            Ok(file) => match rodio::Decoder::new(std::io::BufReader::new(file)) {
                                Ok(source) => sink.append(source),
                                Err(e) => {
                                    crate::rlog!("gomaju: could not decode chime {} ({e})", path.display())
                                }
                            },
                            Err(e) => {
                                crate::rlog!("gomaju: could not open chime {} ({e})", path.display())
                            }
                        }
                    });
                    return;
                }
                _ => {} // malformed -> fall through to the default tone
            }
        }
    }
    play_countdown(move |sink| {
        sink.set_volume(volume_pct as f32 / 100.0);
        fill_alarm(sink);
    });
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

static PREVIEW: LazyLock<Mutex<PreviewState>> =
    LazyLock::new(|| Mutex::new(PreviewState::default()));

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
///
/// Superseding also emits `preview-ended` for the *previous* generation, so a button in **another**
/// window that was showing ⏸ for it reverts to ▶. This is emitted unconditionally (not gated on a
/// live `sink`): a preview registers its sink only later, inside its own thread, so a fast supersede
/// can land while `p.sink` is still `None` — gating on it would leave that window's button stuck.
/// Gen-matching on the JS side means only the matching button reverts; a stale/duplicate emit no-ops.
fn start_preview<F>(app: AppHandle, fill: F) -> u64
where
    F: FnOnce(&Sink) + Send + 'static,
{
    let (gen, superseded) = {
        let mut p = PREVIEW.lock().unwrap();
        let superseded = p.gen; // the generation we're replacing (0 = none has ever played)
        p.gen += 1;
        if let Some(sink) = p.sink.take() {
            sink.stop(); // stop whatever was playing
        }
        (p.gen, superseded)
    };
    if superseded != 0 {
        let _ = app.emit("preview-ended", superseded);
    }
    std::thread::spawn(move || {
        let (_stream, handle) = match OutputStream::try_default() {
            Ok(out) => out,
            Err(e) => {
                crate::rlog!("gomaju: no audio output device ({e})");
                finish_preview(&app, gen);
                return;
            }
        };
        let sink = match Sink::try_new(&handle) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                crate::rlog!("gomaju: could not create audio sink ({e})");
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
                Err(e) => crate::rlog!("gomaju: could not decode chime {} ({e})", path.display()),
            },
            Err(e) => crate::rlog!("gomaju: could not open chime {} ({e})", path.display()),
        }
    })
}

/// Which built-in tone "Default" (an empty/unknown chime id) maps to, per picker context. Mirrors
/// the fallbacks of `play_break_chime` / `play_break_over_chime` / `play_alarm_chime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultTone {
    BreakStart,
    BreakOver,
    Alarm,
}

impl DefaultTone {
    fn fill(self) -> fn(&Sink) {
        match self {
            DefaultTone::BreakStart => fill_break_start,
            DefaultTone::BreakOver => fill_break_over,
            DefaultTone::Alarm => fill_alarm,
        }
    }
}

/// Preview a built-in default tone (stoppable). Used when a picker is on "Default" (empty id).
pub fn preview_default(app: AppHandle, tone: DefaultTone, volume_pct: u8) -> u64 {
    start_preview(app, move |sink| {
        sink.set_volume(volume_pct as f32 / 100.0);
        tone.fill()(sink);
    })
}

/// Stoppable mirror of `play_assigned_or`: preview the saved chime referenced by `chime_id`, or the
/// context's built-in `tone` when it's unassigned/missing/malformed, at `volume_pct`. File names are
/// re-checked with `is_safe_filename` and joined only under `chimes_dir`. Returns the generation
/// token (0 only if nothing could play).
pub fn preview_assigned_or(
    app: AppHandle,
    chime_id: &str,
    volume_pct: u8,
    chimes: &[ChimeDto],
    chimes_dir: &Path,
    tone: DefaultTone,
) -> u64 {
    if chime_id == NONE_CHIME_ID {
        return 0; // explicit "None" -> nothing to preview (the picker button stays idle)
    }
    if !chime_id.is_empty() {
        if let Some(c) = chimes.iter().find(|c| c.id == chime_id) {
            match c.kind {
                ChimeKindDto::Tones if !c.steps.is_empty() => {
                    return preview_chime_spec(app, c.steps.clone(), volume_pct);
                }
                ChimeKindDto::File if is_safe_filename(&c.file) => {
                    return preview_chime_file(app, chimes_dir.join(&c.file), volume_pct);
                }
                _ => {} // malformed -> fall through to the default tone
            }
        }
    }
    preview_default(app, tone, volume_pct)
}
