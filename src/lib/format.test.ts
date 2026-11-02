import { describe, expect, it } from "vitest";
import { formatMb, versionLess } from "./format";

describe("format", () => {
  it("formats megabytes", () => {
    expect(formatMb(180 * 1024 * 1024)).toBe("~180 MB");
    expect(formatMb(0)).toBe("—");
  });

  it("compares shipwright versions", () => {
    expect(versionLess("0.11.0", "0.11.1")).toBe(true);
    expect(versionLess("0.11.1", "0.11.0")).toBe(false);
  });
});
