import type { ReactNode } from "react";
import { headerTagline } from "../lib/version";
import type { Screen } from "../types";
import { ShieldLogo } from "./ShieldLogo";
import { StepPills } from "./StepPills";

export function AppShell({
  screen,
  children,
  footer,
  banner,
  bannerDetail,
  subtitle,
}: {
  screen: Screen;
  children: ReactNode;
  footer: ReactNode;
  banner?: string | null;
  bannerDetail?: string | null;
  subtitle?: string;
}) {
  return (
    <div className="app-bg flex min-h-screen flex-col text-text">
      <header className="flex items-start justify-between gap-6 px-8 pb-4 pt-6">
        <div className="flex items-center gap-3">
          <ShieldLogo size={40} />
          <div>
            <h1 className="font-body text-xl font-semibold tracking-[0.03em] text-text">
              Arnold Client Mods
            </h1>
            <div className="text-sm text-muted">{subtitle ?? headerTagline()}</div>
          </div>
        </div>
        <StepPills screen={screen} />
      </header>
      {banner ? (
        <div
          className="mx-8 mb-2 rounded-xl border border-gold/40 bg-panel px-4 py-2 text-sm text-gold"
          role="status"
        >
          <div>{banner}</div>
          {bannerDetail ? <div className="mt-1 break-all text-text">{bannerDetail}</div> : null}
        </div>
      ) : null}
      <main className="mx-auto flex w-full max-w-5xl flex-1 flex-col px-8 pb-8" id="main">
        {children}
      </main>
      <footer className="flex items-center justify-end gap-3 px-8 py-5">{footer}</footer>
    </div>
  );
}
