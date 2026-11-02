export function ShieldLogo({ size = 36 }: { size?: number }) {
  return (
    <div
      className="flex shrink-0 items-center justify-center rounded-[10px] bg-arnold font-heading text-white"
      style={{ width: size, height: size, fontSize: size * 0.58 }}
      aria-hidden
    >
      A
    </div>
  );
}
