import { packArtFor } from "../lib/packArt";

export function PackArt({
  art,
  size = 96,
  name,
}: {
  art?: string | null;
  size?: number;
  name: string;
}) {
  const known = packArtFor(art);
  if (!known) {
    return (
      <div
        className="flex shrink-0 items-center justify-center rounded-2xl bg-[#241833] text-center font-heading text-gold"
        style={{ width: size, height: size }}
      >
        {name}
      </div>
    );
  }
  return (
    <img
      src={known.src}
      alt={known.alt}
      width={size}
      height={size}
      className="shrink-0 object-contain"
      style={{ width: size, height: size }}
    />
  );
}
