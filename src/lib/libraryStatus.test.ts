import { describe, expect, it } from "vitest";
import type { InstalledState, PackEntry } from "../types";
import { statusForPack } from "./libraryStatus";

function pack(id: string, displayVersion: string): PackEntry {
  return {
    id,
    name: id,
    minecraft: "26.3",
    displayVersion,
    manifestUrl: "",
    manifestSigUrl: "",
  };
}

describe("per-pack library status", () => {
  it("does not label another pack with the selected pack's install", () => {
    const installed = { manifestVersion: "2026.11.7" } as InstalledState;
    const byPack = {
      "better-adventures-plus": installed,
      "other-pack": null,
    };
    expect(statusForPack(pack("better-adventures-plus", "2026.11.7"), byPack)).toBe("INSTALLED");
    expect(statusForPack(pack("other-pack", "1.0.0"), byPack)).toBe("NOT INSTALLED");
    expect(statusForPack(pack("missing-pack", "1.0.0"), byPack)).toBe("NOT INSTALLED");
  });
});
