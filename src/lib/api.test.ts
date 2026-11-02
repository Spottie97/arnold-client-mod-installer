import { describe, expect, it } from "vitest";
import { asErrorPayload } from "./api";

describe("asErrorPayload", () => {
  it("keeps the unconfigured-build error screen", () => {
    const payload = asErrorPayload({
      code: "E-MANIFEST-KEY",
      title: "This build isn't configured",
      hint: "Download the published installer.",
      detail: "placeholder",
    });
    expect(payload.code).toBe("E-MANIFEST-KEY");
    expect(payload.title).toContain("isn't configured");
  });

  it("parses a JSON string payload", () => {
    const payload = asErrorPayload(
      JSON.stringify({
        code: "E-MANIFEST-KEY",
        title: "This build isn't configured",
        hint: "Download the published installer.",
        detail: "placeholder",
      }),
    );
    expect(payload.title).toContain("isn't configured");
  });
});
