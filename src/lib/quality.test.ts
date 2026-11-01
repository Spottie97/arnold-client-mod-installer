import { describe, expect, it } from "vitest";
import {
  parseQuality,
  qualityRow,
  snowLiteForQuality,
  fallbackQualityTable,
} from "./quality";

describe("quality", () => {
  it("accepts low and high aliases", () => {
    expect(parseQuality("low")).toBe("lite");
    expect(parseQuality("high")).toBe("ultra");
    expect(parseQuality("potato")).toBe("potato");
  });

  it("maps potato and lite to lighter snow", () => {
    expect(snowLiteForQuality("potato")).toBe(true);
    expect(snowLiteForQuality("lite")).toBe(true);
    expect(snowLiteForQuality("normal")).toBe(false);
    expect(snowLiteForQuality("ultra")).toBe(false);
  });

  it("keeps ship-heavy ocean off potato", () => {
    const row = qualityRow(fallbackQualityTable(), "potato");
    expect(row.skipComponents).toContain("wakes");
    expect(row.skipComponents).toContain("waves");
    expect(row.description.length).toBeGreaterThan(20);
  });
});
