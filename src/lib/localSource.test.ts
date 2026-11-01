import { describe, expect, it } from "vitest";
import {
  LOCAL_SOURCE_BANNER,
  localSourceBanner,
  pickBanner,
  type LocalSourceInfo,
} from "./localSource";

describe("local source banner", () => {
  it("is the exact test-build sentence", () => {
    expect(LOCAL_SOURCE_BANNER).toBe("TEST BUILD - local source");
  });

  it("shows on every local-source build", () => {
    const info: LocalSourceInfo = {
      enabled: true,
      dir: "D:\\\\mods",
      banner: LOCAL_SOURCE_BANNER,
      needsFolder: false,
    };
    expect(localSourceBanner(info)).toBe("TEST BUILD - local source");
    expect(pickBanner(info, "Using the last good mod list (offline).")).toBe(
      "TEST BUILD - local source",
    );
  });

  it("stays hidden in a normal build", () => {
    expect(
      localSourceBanner({
        enabled: false,
        dir: null,
        banner: null,
        needsFolder: false,
      }),
    ).toBeNull();
    expect(pickBanner(null, "Using the last good mod list (offline).")).toBe(
      "Using the last good mod list (offline).",
    );
  });
});
