# Updater signing key

The installer can self-update from `latest.json` on a dedicated rolling GitHub release whose tag is always `app-updater` (kept as a prerelease so it never becomes `/releases/latest`, which serves the mods manifest):

`https://github.com/Spottie97/arnold-client-mod-installer/releases/download/app-updater/latest.json`

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

The app-release workflow also stages that `latest.json` (and any `.sig` files) onto a **draft** `app-updater` prerelease. Publish the `app-v*` draft first so the URLs inside `latest.json` work, then publish `app-updater` as a prerelease. Do not publish `app-updater` as a normal latest release.

## Manifest signing is a different key

The mod list (`manifest.json` + `manifest.json.sig`) is signed with a separate Ed25519 key. It is not the Tauri updater key.

The public half is committed as `PRODUCTION_MANIFEST_PUBLIC_KEY_HEX` in `src-tauri/src/manifest.rs`. Public builds (anything without the `local-source` feature) verify the remote manifest, the cached manifest, and the bundled fixture with that key only. `active_manifest_public_key()` is the single function those three paths use.

`local-source` builds keep verifying with `TEST_MANIFEST_PUBLIC_KEY_HEX`. That is the test flow. Do not use the production seed for it.

### What a maintainer must set

| Name | Where | Required |
|---|---|---|
| `MANIFEST_SIGNING_KEY` | Secret on the GitHub environment `manifest-release` | Yes. 32-byte Ed25519 seed, hex (64 characters). Never a repository or organization secret used by other workflows. |
| `manifest-release` | GitHub environment | Yes. Restrict deployment branches to `main`. Require a reviewer. |

No GitHub Actions variable is required. The public key is compiled into the app from the constant above.

Optional, not a repository variable:

- `ARNOLD_MANIFEST_PUBKEY` — compile-time override (`option_env!`). A non-empty value replaces the committed public key for that build. A release public build refuses the value when it is missing, the all-zero placeholder, or the test public key. `src-tauri/build.rs` fails the release compile in that case, and the app-release workflow checks it before packaging.
- `ARNOLD_ALLOW_TEST_MANIFEST_KEY=1` — debug and dev builds only. If the production key is the placeholder or the test key, this flag lets that build verify with the test key. Release builds ignore the flag.

No workflow except **Publish mods release** may read `MANIFEST_SIGNING_KEY`. Pull request workflows do not use the `manifest-release` environment. The Windows public build does not take a secret: the public key is already in the source.

### Bundled fixture

`fixtures/manifest.v3.json.sig` stays signed with the **test** key so `local-source` builds and unit tests can load it. A public build verifies that fixture with the production key and skips it when the signature does not match. The app then uses the remote manifest, or the last cached manifest that verified with the production key. CI does not re-sign the fixture, so the private seed is never present for an app build.

### Publish a mods release

1. Upload the Arnold jars to a **draft** GitHub release whose tag is the mods tag (for example `mods-2026.11.2`). The jars are not in this repo.
2. On `main`, run **Publish mods release** (`workflow_dispatch`). Inputs are the mods tag and the manifest source path (default `fixtures/manifest.v3.json`).
3. The signing job declares `environment: manifest-release` and also refuses any ref other than `refs/heads/main`. It downloads the draft assets, checks each manifest file hosted on that tag (sha512 and size), and fails on a mismatch. Modrinth files are not in the draft and are not hashed here.
4. It signs the manifest bytes as-is with `MANIFEST_SIGNING_KEY`. The log must not contain the seed.
5. It uploads `manifest.json` and `manifest.json.sig` onto that same draft. It does not create a release and it does not publish the draft.

GitHub only serves `/releases/latest` from the newest **published** release (not a draft, not a prerelease). The installer requests:

`https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/manifest.json`

After a human publishes the mods draft, that URL returns this manifest while the mods release is the newest published release. If a newer `app-v*` release does not contain `manifest.json`, `/latest` will not serve the mod list. Publish the mods release after that app release, or attach the same two manifest files to the release that should remain latest.

### Rotate the manifest key

Do this on a trusted machine. Do not commit the seed, do not paste it into a pull request, and do not print it in a workflow log.

```bash
umask 077
openssl genpkey -algorithm ED25519 -out "$HOME/arnold-manifest.pem"
# 32-byte seed, hex. This is MANIFEST_SIGNING_KEY. Store the file, then delete it from the shell history.
openssl pkey -in "$HOME/arnold-manifest.pem" -outform DER | tail -c 32 | xxd -p -c 64
# Public key, hex. This is safe to commit.
openssl pkey -in "$HOME/arnold-manifest.pem" -pubout -outform DER | tail -c 32 | xxd -p -c 64
```

1. Replace `PRODUCTION_MANIFEST_PUBLIC_KEY_HEX` in `src-tauri/src/manifest.rs` with the new public key. `build.rs` and the app-release check read that constant. Ship a new public app build so installed copies embed it.
2. Update the `MANIFEST_SIGNING_KEY` secret on the `manifest-release` environment to the new seed. Remove the old seed.
3. From `main`, run **Publish mods release** so the draft gets a manifest signed by the new seed. A human publishes the draft.
4. Installers that still have the old public key reject the new manifest. Installers with the new public key reject manifests signed by the old seed. Ship the app and the signed manifest together.

Confirm the new public key is not `TEST_MANIFEST_PUBLIC_KEY_HEX` and not 64 zeroes. A release build will not load manifests if it is.
