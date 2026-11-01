import type { Screen } from "../types";

const STEPS: { id: Screen; label: string }[] = [
  { id: "welcome", label: "Welcome" },
  { id: "scan", label: "Scan" },
  { id: "kit", label: "Kit" },
  { id: "progress", label: "Install" },
  { id: "done", label: "Done" },
];

function activeIndex(screen: Screen): number {
  switch (screen) {
    case "welcome":
    case "help":
    case "manage":
      return 0;
    case "scan":
      return 1;
    case "kit":
    case "custom":
      return 2;
    case "progress":
    case "error":
      return 3;
    case "done":
      return 4;
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
            className={`pill font-heading ${
              active ? "bg-accent text-white" : "bg-[#2a2438] text-muted"
            }`}
          >
            {step.label}
          </span>
        );
      })}
    </nav>
  );
}
