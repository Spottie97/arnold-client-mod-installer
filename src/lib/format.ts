export function formatMb(bytes: number): string {
  if (bytes <= 0) return "—";
  const mb = bytes / (1024 * 1024);
  if (mb < 1) return `~${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `~${Math.round(mb)} MB`;
}

export function formatWhen(stamp: string): string {
  if (stamp.startsWith("unix-")) {
    const secs = Number(stamp.slice(5));
    if (!Number.isNaN(secs)) {
      return new Date(secs * 1000).toLocaleString();
    }
  }
  return stamp;
}

export function versionLess(a: string, b: string): boolean {
  const parts = (raw: string) =>
    raw
      .split(/[^\d]+/)
      .filter(Boolean)
      .map((n) => Number(n));
  const left = parts(a);
  const right = parts(b);
  const len = Math.max(left.length, right.length);
  for (let i = 0; i < len; i += 1) {
    const l = left[i] ?? 0;
    const r = right[i] ?? 0;
    if (l !== r) return l < r;
  }
  return false;
}
