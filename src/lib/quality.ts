import type { QualityLevel, QualityRow, QualityTable } from "../types";

export const QUALITY_ORDER: QualityLevel[] = ["potato", "lite", "normal", "ultra"];

export function isQualityLevel(value: string | null | undefined): value is QualityLevel {
  return value === "potato" || value === "lite" || value === "normal" || value === "ultra";
}

export function parseQuality(value: string | null | undefined): QualityLevel | null {
  if (!value) return null;
  const raw = value.trim().toLowerCase();
  if (isQualityLevel(raw)) return raw;
  if (raw === "low") return "lite";
  if (raw === "high") return "ultra";
  return null;
}

export function snowLiteForQuality(level: QualityLevel): boolean {
  return level === "potato" || level === "lite";
}

export function fallbackQualityTable(): QualityTable {
  return {
    potato: {
      label: "Potato",
      description:
        "Fastest. Shaders stay off. Sodium uses FAST leaves and weather, a short view distance, and simple clouds. Ships hide spray and foam.",
      skipComponents: ["wakes", "waves"],
      recommendOcean: false,
    },
    lite: {
      label: "Lite",
      description:
        "Light. Complementary on its Low profile. Sodium stays modest. Ships run at half particles.",
      skipComponents: [],
      recommendOcean: false,
    },
    normal: {
      label: "Normal",
      description:
        "Balanced. Complementary on its Medium profile. Ocean extras are welcome. Ships use the defaults Arnold ships.",
      skipComponents: [],
      recommendOcean: true,
    },
    ultra: {
      label: "Ultra",
      description:
        "Heaviest. Complementary on its High profile. Extra ship particles when Arnold sends those values.",
      skipComponents: [],
      recommendOcean: true,
    },
  };
}

export function qualityRow(table: QualityTable | undefined, level: QualityLevel): QualityRow {
  const source = table ?? fallbackQualityTable();
  switch (level) {
    case "potato":
      return source.potato;
    case "lite":
      return source.lite;
    case "normal":
      return source.normal;
    case "ultra":
      return source.ultra;
    default: {
      const _never: never = level;
      return _never;
    }
  }
}
