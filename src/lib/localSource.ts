export const LOCAL_SOURCE_BANNER = "TEST BUILD - local source";

export interface LocalSourceInfo {
  enabled: boolean;
  dir?: string | null;
  banner?: string | null;
  needsFolder: boolean;
}

export function localSourceBanner(info: LocalSourceInfo | null | undefined): string | null {
  if (!info?.enabled) return null;
  return info.banner?.trim() || LOCAL_SOURCE_BANNER;
}

export function pickBanner(
  local: LocalSourceInfo | null | undefined,
  offline: string | null | undefined,
): string | null {
  return localSourceBanner(local) ?? offline ?? null;
}
