import { describe, expect, it } from "vitest";
import { mockFolderSwitch } from "../mock/backend";
import { folderSwitchNotice } from "./folderSwitch";

function packName(id: string): string {
  if (id === "better-adventures-plus") return "Better Adventures+";
  if (id === "pack-x") return "Pack X";
  return id;
}

describe("folderSwitchNotice", () => {
  it("describes a legacy folder that was saved first", () => {
    const text = folderSwitchNotice(
      mockFolderSwitch("better-adventures-plus", "pack-x", true),
      packName,
    );
    expect(text).toContain("earlier Better Adventures+ install");
    expect(text).toContain("Worlds, screenshots, options and servers stayed put");
  });

  it("describes a marked pack that was set aside", () => {
    const text = folderSwitchNotice(mockFolderSwitch("pack-a", "pack-x", false), packName);
    expect(text).toContain("pack-a was set aside");
    expect(text).toContain("Worlds, screenshots, options and servers stayed put");
  });

  it("adds restored, flagged, and partial suffixes", () => {
    const entry = mockFolderSwitch("pack-a", "pack-x", false);
    entry.switch = {
      ...entry.switch!,
      restored: true,
      flagged: ["Not part of pack-a: mods/user.jar left in place"],
      partialRestore: true,
      conflicts: ["mods/x.jar"],
    };
    const text = folderSwitchNotice(entry, packName);
    expect(text).toContain("Pack X's saved mods were put back.");
    expect(text).toContain("1 file(s) that pack-a didn't install were left in place.");
    expect(text).toContain("1 of Pack X's saved files were kept aside");
    expect(text).toContain("won't replace those files without asking");
  });

  it("does not mention saved copies that were kept", () => {
    const entry = mockFolderSwitch("pack-a", "pack-x", false);
    entry.switch = {
      ...entry.switch!,
      flagged: [],
      savedCopies: ["arnold-installer/packs/pack-a.replaced-1"],
    };
    const text = folderSwitchNotice(entry, packName) ?? "";
    expect(text).not.toContain("left in place");
  });

  it("stays quiet when the folder was only claimed for the same pack", () => {
    const entry = mockFolderSwitch("better-adventures-plus", "better-adventures-plus", true);
    entry.switch = null;
    entry.adopted = true;
    expect(folderSwitchNotice(entry, packName)).toBeNull();
  });
});
