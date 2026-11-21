import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { PACK_ART } from "./packArt";

describe("pack art", () => {
  it("credits the official Matcha Flavoured icon", () => {
    const art = PACK_ART["matcha-flavoured"];
    expect(art.credit).toContain("Klei Wright");
    expect(art.licence).toBe("CC BY-NC-SA 4.0");
    expect(art.licenceLink).toBe("cc-by-nc-sa-4");
    expect(art.sourceLink).toBe("matcha-source");
    const bytes = readFileSync("src/assets/packs/matcha-flavoured.png");
    expect(createHash("sha256").update(bytes).digest("hex")).toBe(
      "f80b5fdc282c0db52220f0586695b7626dce8c4722e1f74b467f076f358b4bc6",
    );
  });
});
