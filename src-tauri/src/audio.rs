use std::time::Duration;

use rodio::source::{SineWave, Source};
use rodio::{OutputStream, Sink};

/// Play a gentle two-note chime on a detached thread. The tone is generated in
/// code (no bundled audio asset) at low amplitude with a short fade-in, so it
/// reads as a soft cue rather than a harsh beep. Failures (no output device) are
/// logged and ignored — sound is never essential.
pub fn play_chime() {
    std::thread::spawn(|| {
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

        let note = |freq: f32, ms: u64| {
            SineWave::new(freq)
                .take_duration(Duration::from_millis(ms))
                .amplify(0.18)
                .fade_in(Duration::from_millis(40))
        };
        sink.append(note(659.25, 200)); // E5
        sink.append(note(987.77, 320)); // B5

        // Keep `_stream` alive until playback finishes.
        sink.sleep_until_end();
        eprintln!("restee: chime played");
    });
}
