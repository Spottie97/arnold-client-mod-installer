# Testing with a local mods folder

This is a **test build**. It is not a public release. The `local-source` cargo
feature stays off in everyday builds and is never passed to the app-release
workflow.

Use it when draft GitHub Release files need a login and you do not want a
public pre-release. Put the signed mods folder next to the test exe (or next
to the folder that contains it) and double-click. The app uses that folder
without asking. The folder picker appears only when nothing is found.

The installer still verifies the **test** Ed25519 signature and every SHA-512.
Only files inside the chosen folder may use `file://`. Everything else still
goes through the HTTPS allowlist (Modrinth, this project's Releases, Fabric
meta for the loader).

## Banner

A test build always shows:

`TEST BUILD - local source`

## Get the Windows test installer

1. Open the pull request (or run the **Windows local-source test build**
   workflow by hand).
2. Download the artifact named
   `arnold-client-mods-windows-x64-local-source`.
3. Inside it: `arnold-client-mods.exe` and the `.msi`.

The workflow only uploads that artifact. It does not create a release or a tag.

## Get the Mac test build

1. Open the pull request (or run **macOS public build** / **macOS local-source test build** by hand).
2. Download `arnold-client-mods-macos-universal` (public) or
   `arnold-client-mods-macos-universal-local-source` (test build).
   Both run on Apple Silicon and Intel Macs.
3. Inside: `Arnold Client Mods_3.3.0_universal.dmg` and
   `Arnold-Client-Mods-macos-universal.app.zip`.

These builds are **not signed or notarized** (there is no Apple Developer ID yet), so
macOS Gatekeeper blocks the first launch. Do one of:

- Finder → right-click **Arnold Client Mods** → **Open** → **Open**.
- macOS 15 or newer, if there is no Open button: try to open it once, then
  **System Settings → Privacy & Security → Open Anyway**.
- Terminal:
  `xattr -dr com.apple.quarantine "/Applications/Arnold Client Mods.app"`
  (use the real path if the app is not in Applications).

### Local-source on a Mac

Copy the app out of the dmg into a normal folder, put the signed tester folder
beside it, and clear quarantine so macOS does not run it from a hidden copy:

    ~/ArnoldTest/Arnold Client Mods.app
    ~/ArnoldTest/mods-2026.11.8/manifest.json
    ~/ArnoldTest/mods-2026.11.8/manifest.json.sig

    xattr -dr com.apple.quarantine ~/ArnoldTest/"Arnold Client Mods.app"

If the folder is not found, the app asks for it. You can also run
`open -a ~/ArnoldTest/"Arnold Client Mods.app" --args --local-source ~/ArnoldTest/mods-2026.11.8`.

### Where it installs on a Mac

The default is a pack folder, `~/Library/Application Support/arnold-packs/better-adventures-plus`
(`%AppData%\arnold-packs\better-adventures-plus` on Windows,
`~/.local/share/arnold-packs/better-adventures-plus` on Linux). The Minecraft folder is used
only when the player chooses it. Close the Minecraft Launcher before installing so it does
not overwrite `launcher_profiles.json`.

### A signed pack index

A folder with `manifest.json` and `manifest.json.sig` still works. To test more than one
pack, add `packs.json` and sign it with the test key:

```bash
cargo run --bin release-manifest -- --in <dir>/packs.json --sign-as-is --test-key
```

That writes `<dir>/packs.json.sig`. Relative `manifestUrl` values must stay inside the folder.

## Build it on Windows

From a checkout, with Node 22 and the stable Rust toolchain:

```bat
npm ci
npm run build:local
```

That is `tauri build --features local-source --bundles msi`. The exe lands at
`src-tauri\target\release\arnold-client-mods.exe`.

For a live window:

```bat
npm run dev:local
```

## Prepare a folder of jars

Put the Arnold jars and settings in one folder, for example
`D:\minecraft-mods\release-mods-2026.11.2\`:

- `arnold-ships-client-0.10.3.jar`
- `arnold-deck-client-0.11.1.jar`
- `arnold-ships-client.properties`

Sodium / Iris / Complementary can stay on Modrinth. Files that are actually in
the folder are read from disk; missing names keep their signed HTTPS URLs.

## Sign the folder (test key)

From the repo, on any machine with Rust:

```bat
tools\release-manifest --from-folder D:\minecraft-mods\release-mods-2026.11.2
```

On Linux / macOS:

```bash
tools/release-manifest --from-folder /path/to/release-mods-2026.11.2
```

`--from-folder` hashes the jars and settings that are present, writes
`manifest.json`, `manifest.json.sig`, and `SHA512SUMS.txt` into that same
folder, and signs with the **test** key unless you pass `--key-hex`.

Do not use a production signing key for a local-source test. A public build (no `local-source`) verifies manifests with the production key only and will not accept this test signature. See `docs/updater-key.md`.

## Run it

The test installer is a **release** build (`tauri build`, not `tauri dev`), so
the window opens without a console.

If you do not pass `--local-source` and `ARNOLD_LOCAL_SOURCE` is unset, the app
looks for a subfolder that contains both `manifest.json` and
`manifest.json.sig`:

1. In the exe's own directory.
2. In the parent of that directory.

On a Mac, a `.app` also looks in the folder that contains the app.

Names starting with `mods-` win. If several match, the newest name is used.
The chosen path is shown under the `TEST BUILD - local source` banner, and the
folder picker stays closed.

A layout that works for a double-click test:

```text
D:\minecraft-mods\arnold-client-mods.exe
D:\minecraft-mods\mods-2026.11.2\manifest.json
D:\minecraft-mods\mods-2026.11.2\manifest.json.sig
```

An MSI install puts the exe under Program Files. Auto-detect will not see a
mods folder that was left next to the downloaded zip, so the picker still
appears there. Point it at the folder, or start the exe with a path.

Or start it with a path:

```bat
arnold-client-mods.exe --local-source D:\minecraft-mods\release-mods-2026.11.2
```

```bat
set ARNOLD_LOCAL_SOURCE=D:\minecraft-mods\release-mods-2026.11.2
arnold-client-mods.exe
```

`--local-source` wins over the environment variable. The Welcome screen can
also change the folder later.

## Checks

- The gold banner reads `TEST BUILD - local source`.
- A bad `manifest.json.sig` is rejected (`E-SIG-INVALID`).
- A jar whose SHA-512 does not match the signed list is discarded
  (`E-HASH-MISMATCH`).
- A `file://` path that leaves the folder is rejected (`E-URL-NOT-ALLOWED`).
- Diagnostics show `manifestSource: local` and `manifest: 2026.11.8` when the folder is in use,
  and `manifestSource: bundled` when no folder is chosen (install stays blocked).
  The public installer always uses the live signed list and ignores this folder.

## What this is not

- Not a public release.
- Not enabled on `app-v*` tags.
- Not a way to skip checksums or the test-key signature.
