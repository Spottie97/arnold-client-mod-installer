import avalon from "../assets/brand/avalon-icon.png";

export function ShieldLogo({ size = 40 }: { size?: number }) {
  return (
    <img
      src={avalon}
      alt=""
      width={size}
      height={size}
      className="shrink-0 rounded-full object-contain"
      style={{ width: size, height: size }}
    />
  );
}
