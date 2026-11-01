# Testing with a local mods folder

This is a **test build**. It is not a public release. The `local-source` cargo
feature stays off in everyday builds and is never passed to the app-release
workflow.

Use it when draft GitHub Release files need a login and you do not want a
public pre-release. Reinhardt can double-click the Windows test installer,
pick the folder that holds the jars, and install from disk.

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
`D:\minecraft-mods\release-mods-2026.11.1\`:

- `arnold-snowfall-1.3.3.jar`
- `arnold-ships-client-0.10.2.jar`
- `arnold-deck-client-0.11.1.jar`
- `arnold-ships-client.properties`
- `arnold-snow-client.properties` (`quality=normal`, `fpsTarget=60`, `adaptiveFps=true`)
- `arnold-snow-client.lite.properties` (starts with `# arnold-installer-lite`, `quality=lite`)

Sodium / Iris / Complementary can stay on Modrinth. Files that are actually in
the folder are read from disk; missing names keep their signed HTTPS URLs.

## Sign the folder (test key)

From the repo, on any machine with Rust:

```bat
tools\release-manifest --from-folder D:\minecraft-mods\release-mods-2026.11.1
```

On Linux / macOS:

```bash
tools/release-manifest --from-folder /path/to/release-mods-2026.11.1
```

`--from-folder` hashes the jars and settings that are present, writes
`manifest.json`, `manifest.json.sig`, and `SHA512SUMS.txt` into that same
folder, and signs with the **test** key unless you pass `--key-hex`.

Do not use a production signing key for a local-source test.

## Run it

Double-click the exe or the MSI. If you did not pass a folder, a picker asks
for the directory that contains `manifest.json`.

Or start it with a path:

```bat
arnold-client-mods.exe --local-source D:\minecraft-mods\release-mods-2026.11.1
```

```bat
set ARNOLD_LOCAL_SOURCE=D:\minecraft-mods\release-mods-2026.11.1
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
