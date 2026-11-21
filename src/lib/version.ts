import pkg from "../../package.json";

export const APP_VERSION: string = pkg.version;

export function headerTagline(version: string = APP_VERSION): string {
  return `Optional looks & effects for the Arnold server · Minecraft 26.3 · v${version}`;
}
