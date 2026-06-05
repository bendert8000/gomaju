// Pure music-theory helpers for the chime note composer. No DOM, no Tauri — it converts between the
// editor's note model and the rodio synth's frequency/duration steps, so the UI never deals in Hz/ms.
//
// Pitch is a MIDI note number (C4 = 60). Frequency uses equal temperament with A4 = 440 Hz:
//   f = 440 * 2^((midi - 69) / 12)

export type MajorKey = "C" | "G" | "F";

/** Semitone offsets of the major-scale degrees Do..Si (1..7). */
export const MAJOR_OFFSETS = [0, 2, 4, 5, 7, 9, 11] as const;

/** Semitones above C for each supported key's tonic (Do). */
export const PITCH_CLASS: Record<MajorKey, number> = { C: 0, G: 7, F: 5 };

/** Button labels for scale degrees 1..7 (Do..Si). */
export const SOLFEGE = ["Do", "Re", "Mi", "Fa", "Sol", "La", "Si"] as const;

/** Note-length choices: a music-note `symbol` + its `fraction`; `ms` is the sounded duration. */
export const DURATIONS: { id: string; symbol: string; fraction: string; ms: number }[] = [
  { id: "whole", symbol: "𝅝", fraction: "1/1", ms: 1200 },
  { id: "half", symbol: "𝅗𝅥", fraction: "1/2", ms: 600 },
  { id: "quarter", symbol: "♩", fraction: "1/4", ms: 300 },
  { id: "eighth", symbol: "♪", fraction: "1/8", ms: 150 },
  { id: "sixteenth", symbol: "♬", fraction: "1/16", ms: 75 },
];

const NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** One step of the editor melody. `midi = null` is a rest. */
export interface Note {
  midi: number | null;
  durationMs: number;
}

/** Mirrors restee_core::chime::ToneStep — the host synth's input. Loudness is not per-step; the
 * playback volume is supplied by the rule/alarm picker using the chime. */
export interface ToneStep {
  freq_hz: number;
  duration_ms: number;
  fade_in_ms: number;
}

/** A small fixed fade written into every step so tones don't click. */
const FADE_IN_MS = 15;

/** MIDI note for scale `degree` (1..7) in `key` at `octave` (Do sits at that octave's tonic). */
export function solfegeToMidi(key: MajorKey, degree: number, octave: number): number {
  const tonic = 12 * (octave + 1) + PITCH_CLASS[key]; // C4 = 60
  return tonic + MAJOR_OFFSETS[degree - 1];
}

/** Equal-tempered frequency (Hz) of a MIDI note. */
export function midiToFreq(midi: number): number {
  return 440 * 2 ** ((midi - 69) / 12);
}

/** Nearest MIDI note for a frequency, or null for silence (freq <= 0 = a rest). */
export function freqToMidi(freq: number): number | null {
  if (freq <= 0) return null;
  return Math.round(69 + 12 * Math.log2(freq / 440));
}

/** Scientific-pitch name of a MIDI note, e.g. 72 -> "C5" (sharp spelling). */
export function midiToName(midi: number): string {
  return NAMES[((midi % 12) + 12) % 12] + (Math.floor(midi / 12) - 1);
}

/** The standard length nearest to `ms` (each step's raw ms maps to its closest length). */
function nearestDuration(ms: number): (typeof DURATIONS)[number] {
  let best = DURATIONS[0];
  for (const d of DURATIONS) {
    if (Math.abs(d.ms - ms) < Math.abs(best.ms - ms)) best = d;
  }
  return best;
}

/** The music-note symbol for a duration in ms, for chip display. */
export function durationSymbol(ms: number): string {
  return nearestDuration(ms).symbol;
}

/** The id of the nearest standard length (e.g. "half"), for per-length chip coloring. */
export function durationTier(ms: number): string {
  return nearestDuration(ms).id;
}

/** Editor melody -> synth steps. A rest -> `freq_hz: 0`. Volume is per-chime, not per-step. */
export function notesToSteps(melody: Note[]): ToneStep[] {
  return melody.map((n) => ({
    freq_hz: n.midi == null ? 0 : Math.round(midiToFreq(n.midi)),
    duration_ms: n.durationMs,
    fade_in_ms: FADE_IN_MS,
  }));
}

/** Synth steps -> editor melody. The note name is derived from MIDI; the **exact** ms is preserved
 * (no snapping to a picker value), so reopening + saving an existing chime doesn't alter it. */
export function stepsToNotes(steps: ToneStep[]): Note[] {
  return steps.map((s) => ({ midi: freqToMidi(s.freq_hz), durationMs: s.duration_ms }));
}
