// Small shared helpers for the overlay and toast pages.

/** Format whole seconds as `M:SS`. */
export function fmtMMSS(total: number): string {
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Read a value the backend injected via an initialization script, or `fallback`. */
export function readInjected<T>(name: string, fallback: T): T {
  return (window as unknown as Record<string, T | undefined>)[name] ?? fallback;
}
