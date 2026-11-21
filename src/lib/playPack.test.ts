import { describe, expect, it } from "vitest";
import { playPack } from "./playPack";

describe("playPack", () => {
  it("selects the clicked pack before opening the launcher", async () => {
    const order: string[] = [];
    const prefs = await playPack("other-pack", {
      selectPack: async (id) => {
        order.push(`select:${id}`);
        return { tourDone: true, selectedPack: id };
      },
      play: async () => {
        order.push("play");
        return { opened: true, note: "" };
      },
    });
    expect(order).toEqual(["select:other-pack", "play"]);
    expect(prefs.selectedPack).toBe("other-pack");
  });

  it("does not open the launcher when selection fails", async () => {
    const order: string[] = [];
    await expect(
      playPack("other-pack", {
        selectPack: async () => {
          order.push("select");
          throw new Error("switch failed");
        },
        play: async () => {
          order.push("play");
          return { opened: true, note: "" };
        },
      }),
    ).rejects.toThrow("switch failed");
    expect(order).toEqual(["select"]);
  });
});
