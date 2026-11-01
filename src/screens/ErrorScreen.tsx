import { useState } from "react";
import type { ErrorPayload } from "../types";

export function ErrorScreen({ error }: { error: ErrorPayload }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold md:text-5xl">{error.title}</h1>
      <section className="mt-6 rounded-2xl border border-error bg-panel p-5">
        {error.code === "E-MC-MISSING" ? (
          <>
            <p className="text-lg leading-relaxed">
              Open the Minecraft Launcher, press <strong>Play</strong> once on{" "}
              <strong>Latest release (26.3)</strong>, close the game, then press Try again.
            </p>
            <p className="mt-3 text-muted">
              Nothing was changed on your PC. Error code {error.code}.
            </p>
          </>
        ) : (
          <>
            <p className="text-lg leading-relaxed">{error.hint}</p>
            <p className="mt-3 text-muted">Error code {error.code}.</p>
          </>
        )}
      </section>
      <button
        type="button"
        className="mt-5 self-start text-left text-muted"
        onClick={() => setOpen((value) => !value)}
      >
        {open ? "▼" : "▸"} Details
      </button>
      {open ? (
        <pre className="panel mt-2 overflow-x-auto whitespace-pre-wrap text-sm text-muted">
          {error.detail}
        </pre>
      ) : null}
    </div>
  );
}
