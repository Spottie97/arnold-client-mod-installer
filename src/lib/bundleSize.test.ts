import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// The universal macOS dmg must stay under the 10 MB release gate. Frontend
// assets are embedded once per architecture, and the icns ships twice
// (app icon + dmg volume icon), so these files count double.
describe("bundle size", () => {
  it("serves the 40 px header logo from the 128 px Avalon derivative", () => {
    const logo = readFileSync("src/components/ShieldLogo.tsx", "utf8");
    expect(logo).toContain('from "../assets/brand/avalon-icon-128.png"');
    expect(logo).not.toContain('brand/avalon-icon.png"');
    const bytes = readFileSync("src/assets/brand/avalon-icon-128.png");
    expect(bytes.length).toBeLessThan(64 * 1024);
    expect(bytes).toEqual(readFileSync("src-tauri/icons/128x128.png"));
  });

  it("keeps the macOS icns without duplicate @2x slots", () => {
    const icns = readFileSync("src-tauri/icons/icon.icns");
    const kinds: string[] = [];
    for (let i = 8; i < icns.length; ) {
      kinds.push(String.fromCharCode(...icns.subarray(i, i + 4)));
      const size = (icns[i + 4] << 24) | (icns[i + 5] << 16) | (icns[i + 6] << 8) | icns[i + 7];
      i += size;
    }
    expect(kinds).toContain("ic10");
    expect(kinds).not.toContain("ic13");
    expect(kinds).not.toContain("ic14");
    expect(icns.length).toBeLessThan(2 * 1024 * 1024);
  });
});
