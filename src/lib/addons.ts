import type { Component, Manifest } from "../types";

function isComponent(component: Component | undefined): component is Component {
  return component != null;
}

export interface AddonRow {
  id: string;
  name: string;
  detail: string;
  note: string | null;
  checked: boolean;
}

function concreteAddonComponents(manifest: Manifest, componentIds: string[]): string[] {
  return componentIds.filter((id) => {
    const component = manifest.components.find((item) => item.id === id);
    return component != null && component.kind !== "pending";
  });
}

/**
 * Flags for a saved install. `null` means nothing is installed yet, so the
 * caller may use each addon's defaultOn. A record means a selection already
 * exists: an addon id that is absent or false stays off, even when defaultOn
 * is true. Ocean is not included; the hardware recommendation still owns it.
 */
export function savedAddonFlags(
  manifest: Manifest,
  installedIds: readonly string[] | null,
): Record<string, boolean> | null {
  if (installedIds == null) return null;
  const have = new Set(installedIds);
  const flags: Record<string, boolean> = {};
  for (const [id, addon] of Object.entries(manifest.addons ?? {})) {
    if (id === "ocean") continue;
    const concrete = concreteAddonComponents(manifest, addon.components);
    flags[id] = concrete.length > 0 && concrete.every((componentId) => have.has(componentId));
  }
  return flags;
}

/** Fresh choice when no selection is saved. An addon turns on only when every kit opts in. */
export function freshAddonFlags(
  manifest: Manifest,
  saved: Record<string, boolean> | null,
): Record<string, boolean> {
  const kits = Object.keys(manifest.kits);
  const flags: Record<string, boolean> = {};
  for (const [id, addon] of Object.entries(manifest.addons ?? {})) {
    if (id === "ocean") continue;
    if (saved) {
      flags[id] = saved[id] === true;
      continue;
    }
    flags[id] = kits.length > 0 && kits.every((kit) => addon.defaultOn?.[kit] === true);
  }
  return flags;
}

export function addonRows(
  manifest: Manifest,
  ocean: boolean,
  flags: Record<string, boolean>,
): AddonRow[] {
  const kits = Object.keys(manifest.kits);
  return Object.entries(manifest.addons ?? {}).map(([id, addon]) => {
    const components = addon.components
      .map((componentId) => manifest.components.find((item) => item.id === componentId))
      .filter(isComponent);
    const offForEveryKit =
      kits.length > 0 && kits.every((kit) => addon.defaultOn?.[kit] === false);
    const primary = components.find((component) => component.kind !== "pending") ?? components[0];
    return {
      id,
      name: addon.name,
      detail: components
        .map((component) =>
          component.kind === "pending" ? `${component.name} (coming soon)` : component.name,
        )
        .filter((name) => name !== addon.name)
        .join(" · "),
      note: offForEveryKit ? (primary?.description ?? null) : null,
      checked: id === "ocean" ? ocean : flags[id] === true,
    };
  });
}
