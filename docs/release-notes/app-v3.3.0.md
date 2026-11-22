Arnold Client Mods 3.3.0. This is the first installer release after 3.2.0. It ships with mod list 2026.11.8.

- Removed the Snowfall add-on from every kit and from Custom.
- Leftover Snowfall jars (`arnold-snowfall*.jar`, `arnold-snow-client*.jar`) in `mods` are deleted by name on the next install or update. Each one is saved to the restore point first, so a rollback brings it back.
- Better Snow Coverage stays in Full and Recommended.
- Serene Seasons and GlitchCore are in every kit, including Basic.
- Also locked in every kit: Drink Beer Refill, Farmer's Delight, Balm, Hardcore Revival, Shogi, Waystones, Cultural Delights, Xaero's Minimap and World Map, Traveler's Backpack and Animal Feeding Trough. Shippy Ships and Ingenium API are included as staging candidates. All of them come from the Modrinth CDN, pinned by sha512.
- Better Adventures+ 1.64 is unchanged: the server-side mods are in every kit, and the client mods and six resource packs are in Full and Recommended. Diet+ is in every kit. AppleSkin and the client cosmetics (Unbound Third Person, Plane Advancements, Audio Hotkeys, Weather Glass) are in Full and Recommended.
- Arnold land client and Safe Zone are not part of any kit.

Publish this release before marking mods-2026.11.8 as latest. 3.2.0 does not remove leftover Snowfall jars, and it does not enforce `minAppVersion`. Players on 3.2.0 should update to 3.3.0 through the in-app updater first.
