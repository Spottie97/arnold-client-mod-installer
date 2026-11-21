import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const BANNED = [
  "Optional looks & effects",
  "Optional looks and effects",
  "Vanilla clients can still join",
  "Vanilla still works",
  "Lite kit",
  "Horizon",
  "Pack art is a typographic placeholder",
  "tlauncher.org",
  "Flashback stays",
];

function filesIn(dir: string): string[] {
  return readdirSync(dir)
    .filter((name) => name.endsWith(".tsx") || name.endsWith(".ts"))
    .map((name) => join(dir, name));
}

describe("wording", () => {
  it("keeps banned phrases out of the installer copy", () => {
    const files = [
      ...filesIn("src/screens"),
      ...filesIn("src/components"),
      "src/lib/version.ts",
      "src/mock/backend.ts",
    ];
    const hits = files.flatMap((file) => {
      const text = readFileSync(file, "utf8");
      return BANNED.filter((phrase) => text.includes(phrase)).map((phrase) => `${file}: ${phrase}`);
    });
    expect(hits).toEqual([]);
  });

  it("names the three kits and the four quality levels", () => {
    const kit = readFileSync("src/screens/Kit.tsx", "utf8");
    expect([...kit.matchAll(/title="([^"]+)"/g)].map((match) => match[1])).toEqual([
      "Basic",
      "Recommended",
      "Full",
    ]);
    const quality = readFileSync("src/lib/quality.ts", "utf8");
    for (const label of ["Potato", "Lite", "Normal", "Ultra"]) {
      expect(quality).toContain(`label: "${label}"`);
    }
  });
});
