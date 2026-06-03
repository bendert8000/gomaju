use std::time::Duration;

use rodio::source::{SineWave, Source};
use rodio::{OutputStream, Sink};

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
