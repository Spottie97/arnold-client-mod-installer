import pkg from "../../package.json";

export const APP_VERSION: string = pkg.version;

export function headerTagline(version: string = APP_VERSION): string {
  return `For the Arnold server · Minecraft 26.3 · v${version}`;
}

export function packSubtitle(pack: string, detail: string, version: string = APP_VERSION): string {
  return `${pack} · ${detail} · v${version}`;
}
