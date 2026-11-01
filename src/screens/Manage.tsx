import type { InstalledState, SnapshotMeta } from "../types";

export function Manage({
  installed,
  snapshots,
}: {
  installed: InstalledState | null;
  snapshots: SnapshotMeta[];
}) {
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold">Manage / Uninstall</h1>
      <section className="panel mt-6">
        <h2 className="font-heading text-xl text-gold">Installed</h2>
        <ul className="mt-3 space-y-2">
          {(installed?.components ?? []).map((component) => (
            <li key={component.id} className="flex justify-between text-muted">
              <span>{component.name}</span>
              <span>{component.version}</span>
            </li>
          ))}
        </ul>
      </section>
      <section className="panel mt-4">
        <h2 className="font-heading text-xl text-gold">Restore points</h2>
        <ul className="mt-3 space-y-2 text-muted">
          {snapshots.map((snap) => (
            <li key={snap.id}>
              {snap.createdAt} · {snap.label} · {snap.kit ?? "custom"} · {snap.files.length} files
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
