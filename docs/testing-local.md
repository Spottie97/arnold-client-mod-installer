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

- `arnold-snowfall-1.3.3.jar`
- `arnold-ships-client-0.10.3.jar`
- `arnold-deck-client-0.11.1.jar`
- `arnold-ships-client.properties`
- `arnold-snow-client.properties` (`quality=normal`, `fpsTarget=60`, `adaptiveFps=true`)
- `arnold-snow-client.lite.properties` (starts with `# arnold-installer-lite`, `quality=lite`)

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

## What this is not

- Not a public release.
- Not enabled on `app-v*` tags.
- Not a way to skip checksums or the test-key signature.
