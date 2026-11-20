import type { ProgressEvent } from "../types";

/** Keep one log row per running step/file. A new step, file, or status appends. */
export function mergeProgress(rows: ProgressEvent[], event: ProgressEvent): ProgressEvent[] {
  const last = rows[rows.length - 1];
  if (
    last &&
    last.step === event.step &&
    last.file === event.file &&
    last.status === event.status &&
    event.status === "running"
  ) {
    return [...rows.slice(0, -1), event];
  }
  return [...rows, event];
}
