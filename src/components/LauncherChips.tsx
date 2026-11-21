import { StatusChip } from "./StatusChip";
import type { Detection } from "../types";

export function LauncherChips({ detection }: { detection: Detection }) {
  const official = Boolean(detection.launchers?.official);
  const tlauncher = Boolean(detection.launchers?.tlauncher);
  return (
    <div className="mt-4 flex flex-wrap gap-3">
      <StatusChip
        tone={detection.minecraftVersionInstalled ? "ok" : "warn"}
        label={
          detection.minecraftVersionInstalled
            ? `Minecraft ${detection.minecraftVersion} found`
            : `Minecraft ${detection.minecraftVersion} missing`
        }
      />
      <StatusChip
        tone={official ? "ok" : "idle"}
        label={official ? "Official launcher found" : "Official launcher not found"}
      />
      <StatusChip
        tone={tlauncher ? "ok" : "idle"}
        label={tlauncher ? "TLauncher found" : "TLauncher not found"}
      />
    </div>
  );
}
