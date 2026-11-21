import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { APP_VERSION, headerTagline } from "./version";

describe("header", () => {
  it("uses the body font for the app title", () => {
    const shell = readFileSync("src/components/AppShell.tsx", "utf8");
    expect(shell).toContain("font-body");
    expect(shell).toContain("Arnold Client Mods");
    expect(shell).not.toContain("font-heading text-xl");
    expect(headerTagline()).toBe(`For the Arnold server · Minecraft 26.3 · v${APP_VERSION}`);
  });
});
