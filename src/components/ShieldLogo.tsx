// The header shows the logo at 40 CSS px (80-120 device px on HiDPI screens), so it
// uses the 128 px derivative that `tauri icon` makes from brand/avalon-icon.png.
// The 1024 px master is not imported: every bundled asset is embedded in the app
// binary, twice in the universal macOS build.
import avalon from "../assets/brand/avalon-icon-128.png";

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
