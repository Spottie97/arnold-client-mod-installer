import type { Prefs } from "../types";

export async function playPack(
  id: string,
  actions: {
    selectPack: (id: string) => Promise<Prefs>;
    play: () => Promise<unknown>;
  },
): Promise<Prefs> {
  const prefs = await actions.selectPack(id);
  await actions.play();
  return prefs;
}
