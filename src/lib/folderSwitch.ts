import type { DestinationEntry } from "../types";

export function folderSwitchNotice(
  entry: DestinationEntry,
  packName: (id: string) => string,
): string | null {
  const report = entry.switch;
  if (!report) return null;
  const to = packName(report.to);
  const from = report.from ? packName(report.from) : "";
  let text = "";
  if (report.from) {
    text = report.legacy
      ? `Your earlier ${from} install in this folder was saved first, so ${to} can use it. Worlds, screenshots, options and servers stayed put. Open ${from} again to bring it back.`
      : `${from} was set aside so ${to} can use your Minecraft folder. Worlds, screenshots, options and servers stayed put. Open ${from} again to bring it back.`;
  }
  if (report.restored) {
    text += ` ${to}'s saved mods were put back.`;
  }
  if (report.from && report.flagged.length > 0) {
    text += ` ${report.flagged.length} file(s) that ${from} didn't install were left in place.`;
  }
  if (report.partialRestore) {
    text += ` Some of ${to}'s saved files were kept aside because a different file was already there.`;
  }
  const notice = text.trim();
  return notice.length > 0 ? notice : null;
}
