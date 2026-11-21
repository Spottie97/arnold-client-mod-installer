import type { Screen } from "../types";

const STEPS: { id: Screen; label: string }[] = [
  { id: "welcome", label: "Packs" },
  { id: "pack", label: "Details" },
  { id: "scan", label: "This PC" },
  { id: "kit", label: "Kit" },
  { id: "custom", label: "Options" },
  { id: "progress", label: "Install" },
  { id: "done", label: "Done" },
];

function activeIndex(screen: Screen): number {
  switch (screen) {
    case "welcome":
    case "help":
    case "manage":
    case "import":
    case "settings":
      return 0;
    case "pack":
      return 1;
    case "scan":
      return 2;
    case "kit":
      return 3;
    case "custom":
      return 4;
    case "progress":
    case "error":
      return 5;
    case "done":
      return 6;
    default: {
      const _exhaustive: never = screen;
      return _exhaustive;
    }
  }
}

export function StepPills({ screen }: { screen: Screen }) {
  const current = activeIndex(screen);
  return (
    <nav className="flex flex-wrap justify-end gap-2" aria-label="Installer steps">
      {STEPS.map((step, index) => {
        const active = index === current;
        return (
          <span
            key={step.id}
            className={`pill ${active ? "bg-accent text-white" : "bg-[#2a2438] text-muted"}`}
            aria-current={active ? "step" : undefined}
          >
            {step.label}
          </span>
        );
      })}
    </nav>
  );
}
