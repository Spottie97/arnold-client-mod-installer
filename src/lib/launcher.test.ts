import { describe, expect, it } from "vitest";
import { launcherNote } from "./api";

describe("launcher note", () => {
  it("names the profile the installer writes", () => {
    expect(launcherNote("Better Adventures+", "Minecraft Launcher")).toBe(
      "Open Minecraft Launcher and choose the 'Arnold — Better Adventures+' profile.",
    );
  });
});
