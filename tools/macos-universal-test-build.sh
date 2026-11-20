#!/usr/bin/env bash
# Unsigned CI test artifacts only. Not for signed releases.
#
# `tauri build --target universal-apple-darwin` lipos the app binary and
# leaves other Cargo [[bin]] targets in the per-arch release directories.
# The bundler then fails looking for
# src-tauri/target/universal-apple-darwin/release/release-manifest.
# release-manifest stays a normal package bin (Linux CI and
# tools/release-manifest need it). If the universal build fails because
# that bin was not lipo'd, create it from the aarch64 and x86_64 outputs
# and re-run the same build. Tauri does not remove sibling files in the
# universal release dir, so the lipo'd bin is still there when bundling
# runs again. If a lipo input is missing, print the path and exit with
# the original build status without a second build.
set -euo pipefail

cd "$(cd "$(dirname "$0")" && pwd)/.."

universal="src-tauri/target/universal-apple-darwin/release"
arm64="src-tauri/target/aarch64-apple-darwin/release"
x64="src-tauri/target/x86_64-apple-darwin/release"

set +e
npx tauri build --target universal-apple-darwin "$@" --bundles app,dmg
status=$?
set -e
if [[ "${status}" -eq 0 ]]; then
  exit 0
fi

if [[ ! -d "${universal}" ]]; then
  echo "FAIL: ${universal} does not exist." >&2
  echo "tauri build failed before the universal release dir was created; not retrying." >&2
  exit "${status}"
fi

need_retry=0
input_missing=0
# Extra package bins. Tauri already lipos the default-run app binary.
# Add a name here if Cargo.toml grows another [[bin]].
for bin in release-manifest; do
  dest="${universal}/${bin}"
  if [[ -e "${dest}" ]]; then
    continue
  fi
  arm_bin="${arm64}/${bin}"
  x64_bin="${x64}/${bin}"
  if [[ ! -f "${arm_bin}" || ! -f "${x64_bin}" ]]; then
    echo "FAIL: cannot lipo ${bin}; per-arch input missing" >&2
    if [[ ! -f "${arm_bin}" ]]; then
      echo "  missing: ${arm_bin}" >&2
    fi
    if [[ ! -f "${x64_bin}" ]]; then
      echo "  missing: ${x64_bin}" >&2
    fi
    input_missing=1
    continue
  fi
  echo "lipo ${bin} -> ${dest}"
  if ! lipo -create "${arm_bin}" "${x64_bin}" -output "${dest}"; then
    echo "FAIL: lipo -create failed for ${bin}" >&2
    exit 1
  fi
  chmod +x "${dest}"
  if ! archs=$(lipo -archs "${dest}"); then
    echo "FAIL: lipo -archs failed for ${dest}" >&2
    exit 1
  fi
  echo "archs (${bin}): ${archs}"
  case " ${archs} " in
    *" arm64 "*) ;;
    *)
      echo "FAIL: ${dest} is not universal (missing arm64): ${archs}" >&2
      exit 1
      ;;
  esac
  case " ${archs} " in
    *" x86_64 "*) ;;
    *)
      echo "FAIL: ${dest} is not universal (missing x86_64): ${archs}" >&2
      exit 1
      ;;
  esac
  need_retry=1
done

if [[ "${input_missing}" -ne 0 ]]; then
  echo "not retrying the universal bundle" >&2
  exit "${status}"
fi

if [[ "${need_retry}" -eq 0 ]]; then
  echo "tauri build failed and no extra package bin was missing; not retrying" >&2
  exit "${status}"
fi

echo "retrying universal app + dmg bundle"
set +e
npx tauri build --target universal-apple-darwin "$@" --bundles app,dmg
status=$?
set -e
exit "${status}"
