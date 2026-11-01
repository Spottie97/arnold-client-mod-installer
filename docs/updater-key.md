# Updater signing key

The installer can self-update from `latest.json` on this project's GitHub Releases:

`https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/latest.json`

The **private** updater key is a CI secret. It is never committed. A default `cargo build` / `npm run tauri build` works without it; `check_update` then reports that the updater is disabled.

## Generate a key pair

From the repo root, with the Tauri CLI:

```bash
npx @tauri-apps/cli signer generate -w ~/.tauri/arnold-updater.key
```

This writes:

- `~/.tauri/arnold-updater.key` — **private**. Store as GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY`.
- `~/.tauri/arnold-updater.key.pub` — public. Store as `TAURI_UPDATER_PUBKEY` and inject only at release build time.

If the generator asks for a password, store it as `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

## Local / CI builds

```bash
# everyday build — updater compiled out
cargo build --manifest-path src-tauri/Cargo.toml

# release with updater
export TAURI_SIGNING_PRIVATE_KEY
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
export TAURI_UPDATER_PUBKEY
cargo build --manifest-path src-tauri/Cargo.toml --features updater
```

`tauri-action` can emit `latest.json` when those secrets are present (`includeUpdaterJson: true`). Without the secrets, app-v* releases still publish unsigned `.msi` / `.dmg` / slim AppImage artifacts.

## Manifest signing is a different key

Mod-list signatures use `MANIFEST_SIGNING_KEY` (ed25519 hex) with `tools/release-manifest`. That key is also a CI secret. Local fixture signing uses `--test-key` only.
