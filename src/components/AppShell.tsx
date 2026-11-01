import type { ReactNode } from "react";
import type { Screen } from "../types";
import { ShieldLogo } from "./ShieldLogo";
import { StepPills } from "./StepPills";

export function AppShell({
  screen,
  children,
  footer,
}: {
  screen: Screen;
  children: ReactNode;
  footer: ReactNode;
}) {
  return (
    <div className="app-bg flex min-h-screen flex-col text-text">
      <header className="flex items-start justify-between gap-6 px-8 pb-4 pt-6">
        <div className="flex items-center gap-3">
          <ShieldLogo size={40} />
          <div>
            <div className="font-heading text-xl text-text">Arnold Client Mods</div>
            <div className="text-sm text-muted">
              Optional looks &amp; effects for the Arnold server · Minecraft 26.3 · v3.0.0
            </div>
          </div>
        </div>
        <StepPills screen={screen} />
      </header>
      <main className="mx-auto flex w-full max-w-5xl flex-1 flex-col px-8 pb-8">{children}</main>
      <footer className="flex items-center justify-end gap-3 px-8 py-5">{footer}</footer>
    </div>
  );
}
