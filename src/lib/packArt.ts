import matcha from "../assets/packs/matcha-flavoured.png";

export const PACK_ART = {
  "matcha-flavoured": {
    src: matcha,
    alt: "Matcha Flavoured icon",
    credit: "Matcha Flavoured icon © Klei Wright",
    licence: "CC BY-NC-SA 4.0",
    licenceLink: "cc-by-nc-sa-4",
    sourceLink: "matcha-source",
    note: "Matcha Flavoured is a resource pack in this kit; this is not the Better Adventures+ logo.",
  },
} as const;

export type PackArtKey = keyof typeof PACK_ART;

export function packArtFor(key: string | null | undefined) {
  if (!key) return null;
  if (key in PACK_ART) return PACK_ART[key as PackArtKey];
  return null;
}
