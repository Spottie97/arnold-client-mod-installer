import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { APP_VERSION, headerTagline } from "./version";

describe("version", () => {
  it("matches package.json, Cargo.toml, and tauri.conf.json", () => {
    const pkg = JSON.parse(readFileSync("package.json", "utf8")) as { version: string };
    expect(APP_VERSION).toBe(pkg.version);

    const cargo = readFileSync("src-tauri/Cargo.toml", "utf8");
    const cargoVersion = cargo.match(/^version = "([^"]+)"/m)?.[1];
    expect(APP_VERSION).toBe(cargoVersion);

    const tauri = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")) as {
      version: string;
    };
    expect(APP_VERSION).toBe(tauri.version);
  });

  it("builds the header tagline from the app version", () => {
    expect(headerTagline().endsWith(`v${APP_VERSION}`)).toBe(true);
    expect(headerTagline("9.9.9")).toContain("v9.9.9");
  });

  it("does not hard-code a version in the header", () => {
    const shell = readFileSync("src/components/AppShell.tsx", "utf8");
    expect(shell).not.toMatch(/v\d+\.\d+\.\d+/);
  });
});
