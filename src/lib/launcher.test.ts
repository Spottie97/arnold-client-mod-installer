import { describe, expect, it } from "vitest";
import { LAUNCHER_NOTE } from "./api";

describe("launcher note", () => {
  it("names the profile the installer writes", () => {
    expect(LAUNCHER_NOTE).toBe(
      "Open Minecraft and choose the 'Arnold – Fabric 26.3' profile.",
    );
  });
});
