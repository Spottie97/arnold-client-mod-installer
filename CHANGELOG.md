# Changelog

Installer (`app-v*`) releases. Mod-list releases (`mods-*`) are described on their GitHub release.
Releases before 3.3.0 are on the GitHub Releases page.

## 3.3.0

First installer release after 3.2.0. Bundled mod list: manifest **2026.11.8** (`minAppVersion` 3.3.0).

- **Snowfall add-on removed.** The Arnold Snow / Snowfall component is gone from every kit and from Custom.
- **Leftover Snowfall jars are cleaned up.** On the next install or update, `mods/arnold-snowfall*.jar` and
  `mods/arnold-snow-client*.jar` are deleted by name. Each one is saved to the restore point first, so a
  rollback puts it back.
- **Better Snow Coverage stays** in Full and Recommended. It is a Better Adventures+ client mod, not part of Snowfall.
- **Serene Seasons and GlitchCore are in every kit, including Basic**, and stay locked in Custom.
- **Locked in every kit, including Basic:** Drink Beer Refill, Farmer's Delight, Balm, Hardcore Revival, Shogi,
  Waystones, Cultural Delights, Xaero's Minimap and World Map, Traveler's Backpack and Animal Feeding Trough.
  Shippy Ships and Ingenium API are also included as staging candidates. All of them come from the Modrinth CDN
  and are pinned by sha512.
- **Better Adventures+ 1.64 stays as it was:** the server-side BA+ mods are locked in every kit, the BA+ client
  mods are in Full and Recommended, and the six BA+ resource packs go to `resourcepacks` (Full and Recommended).
- **Diet+ 2.2.0** is in every kit. **AppleSkin 3.0.10** is in Full and Recommended.
- **Client cosmetics** stay as they were: Unbound Third Person (no zoom animation), Plane Advancements (no toolbar),
  Audio Hotkeys (no overlay) and Weather Glass 0.6.0, in Full and Recommended.
- Arnold-hosted files (Fabric loader profile, Arnold Decks, Arnold Ships, Diet+ and the cosmetics above) now point
  at the `mods-2026.11.8` release tag. Their bytes, sha512 values and sizes are the same as before.
- The kit list is otherwise the same as before. Arnold land client and Safe Zone are **not** in any kit.
