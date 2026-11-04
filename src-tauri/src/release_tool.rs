use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha512};

/// TEST-ONLY key. Production signing uses MANIFEST_SIGNING_KEY (CI secret).
pub const TEST_PRIVATE_KEY_HEX: &str =
    "48ab0651056657627942de64e86d5435d7391e1ecd922679081c2fbadc376c98";
pub const TEST_PUBLIC_KEY_HEX: &str =
    "b44547ef0ceb7e4bac652ab9b73b46b2ff3d86d27196a479b674be29e87fdbef";
pub const DEFAULT_TAG: &str = "mods-2026.11.4";

fn sha512_hex(data: &[u8]) -> String {
    hex::encode(Sha512::digest(data))
}

pub fn usage_line() -> String {
    format!(
        "release-manifest --in <manifest.json> [--out <dir>] [--assets <dir>] [--from-folder <dir>] [--tag {DEFAULT_TAG}] [--key-hex HEX] [--test-key] [--sign-as-is] [--verify-assets <dir>]"
    )
}

/// Choose the signing seed. `--test-key` / `--from-folder` force the test seed
/// unless `--key-hex` is set. A public sign refuses the test seed.
pub fn resolve_signing_key(
    explicit_hex: Option<&str>,
    env_key: Option<&str>,
    test_key: bool,
) -> Result<String, String> {
    if let Some(hex) = explicit_hex
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(hex.to_string());
    }
    if test_key {
        return Ok(TEST_PRIVATE_KEY_HEX.to_string());
    }
    let Some(env_key) = env_key.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err("pass --key-hex, MANIFEST_SIGNING_KEY, or --test-key".into());
    };
    if env_key == TEST_PRIVATE_KEY_HEX {
        return Err(
            "MANIFEST_SIGNING_KEY is the test key. Refusing to sign a public manifest with it."
                .into(),
        );
    }
    Ok(env_key.to_string())
}

/// Check every GitHub Release file listed in the manifest against bytes in `assets`.
/// Modrinth files are skipped. Missing files and hash or size mismatches fail.
pub fn verify_release_assets(
    manifest_bytes: &[u8],
    assets: &Path,
    tag: &str,
) -> Result<usize, String> {
    let manifest: serde_json::Value =
        serde_json::from_slice(manifest_bytes).map_err(|e| e.to_string())?;
    let prefix = format!(
        "https://github.com/Spottie97/arnold-client-mod-installer/releases/download/{tag}/"
    );
    let components = manifest
        .get("components")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "manifest has no components".to_string())?;
    let mut checked = 0usize;
    for component in components {
        let Some(files) = component.get("files").and_then(|value| value.as_array()) else {
            continue;
        };
        for file in files {
            let name = file
                .get("file")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let url = file
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !url
                .starts_with("https://github.com/Spottie97/arnold-client-mod-installer/releases/")
            {
                continue;
            }
            if name.is_empty() {
                return Err("release asset is missing a file name".into());
            }
            let expected_url = format!("{prefix}{name}");
            if url != expected_url {
                return Err(format!(
                    "{name} url is not {expected_url} (manifest tag must be {tag})"
                ));
            }
            let expected = file
                .get("sha512")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let path = assets.join(name);
            if !path.is_file() {
                return Err(format!("missing release asset {name}"));
            }
            let bytes = fs::read(&path).map_err(|e| format!("{name}: {e}"))?;
            if let Some(size) = file.get("size").and_then(|value| value.as_u64()) {
                if bytes.len() as u64 != size {
                    return Err(format!(
                        "{name} size mismatch: manifest {size}, file {}",
                        bytes.len()
                    ));
                }
            }
            let actual = sha512_hex(&bytes);
            if actual != expected {
                return Err(format!(
                    "{name} sha512 mismatch: manifest {expected}, file {actual}"
                ));
            }
            checked += 1;
        }
    }
    if checked == 0 {
        return Err(format!("no Arnold release assets for tag {tag}"));
    }
    Ok(checked)
}

pub fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

pub fn sign(bytes: &[u8], key_hex: &str) -> Result<[u8; 64], String> {
    let raw = hex::decode(key_hex.trim()).map_err(|e| e.to_string())?;
    let arr: [u8; 32] = raw
        .try_into()
        .map_err(|_| "signing key must be 32 bytes hex".to_string())?;
    let key = SigningKey::from_bytes(&arr);
    Ok(key.sign(bytes).to_bytes())
}

pub fn collect_sha512sums(
    manifest: &serde_json::Value,
    assets: Option<&Path>,
) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if let Some(components) = manifest.get("components").and_then(|v| v.as_array()) {
        for component in components {
            if let Some(files) = component.get("files").and_then(|v| v.as_array()) {
                for file in files {
                    let name = file
                        .get("file")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if name.is_empty() {
                        continue;
                    }
                    if let Some(dir) = assets {
                        let path = dir.join(name);
                        if path.is_file() {
                            let bytes = fs::read(&path).unwrap_or_default();
                            rows.push((sha512_hex(&bytes), name.to_string()));
                            continue;
                        }
                    }
                    if let Some(hash) = file.get("sha512").and_then(|v| v.as_str()) {
                        rows.push((hash.to_string(), name.to_string()));
                    }
                }
            }
        }
    }
    if let Some(dir) = assets {
        for extra in [
            "arnold-ships-client.properties",
            "arnold-snow-client.properties",
            "arnold-snow-client.lite.properties",
            "SHA512SUMS.txt",
            "manifest.json",
        ] {
            let path = dir.join(extra);
            if path.is_file() && !rows.iter().any(|(_, n)| n == extra) {
                let bytes = fs::read(&path).unwrap_or_default();
                rows.push((sha512_hex(&bytes), extra.to_string()));
            }
        }
    }
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    rows
}

pub fn apply_asset_hashes(manifest: &mut serde_json::Value, assets: &Path, tag: &str) {
    let Some(components) = manifest
        .get_mut("components")
        .and_then(|v| v.as_array_mut())
    else {
        return;
    };
    for component in components {
        let Some(files) = component.get_mut("files").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for file in files {
            let name = file
                .get("file")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if name.is_empty() {
                continue;
            }
            let path = assets.join(&name);
            if path.is_file() {
                let bytes = fs::read(&path).unwrap_or_default();
                file["sha512"] = serde_json::Value::String(sha512_hex(&bytes));
                file["size"] = serde_json::Value::from(bytes.len() as u64);
            }
            if name.starts_with("arnold-") {
                file["url"] = serde_json::Value::String(format!(
                    "https://github.com/Spottie97/arnold-client-mod-installer/releases/download/{tag}/{name}"
                ));
            }
        }
    }
}

pub struct WriteResult {
    pub json_path: PathBuf,
    pub used_test_key: bool,
    pub tag: String,
}

/// Hash jars/settings in `folder`, sign with the test key, write manifest.json + .sig there.
pub fn write_from_folder(
    input_json: &Path,
    folder: &Path,
    tag: &str,
) -> Result<WriteResult, String> {
    write_signed(
        input_json,
        folder,
        Some(folder),
        tag,
        TEST_PRIVATE_KEY_HEX,
        false,
    )
}

pub fn write_signed(
    input_json: &Path,
    out: &Path,
    assets: Option<&Path>,
    tag: &str,
    key_hex: &str,
    sign_as_is: bool,
) -> Result<WriteResult, String> {
    let input_bytes = fs::read(input_json).map_err(|e| e.to_string())?;
    let used_test_key = key_hex == TEST_PRIVATE_KEY_HEX;
    if sign_as_is {
        let signature = sign(&input_bytes, key_hex)?;
        let sig_path = PathBuf::from(format!("{}.sig", input_json.display()));
        fs::write(&sig_path, hex::encode(signature)).map_err(|e| e.to_string())?;
        return Ok(WriteResult {
            json_path: input_json.to_path_buf(),
            used_test_key,
            tag: tag.to_string(),
        });
    }

    let mut manifest: serde_json::Value =
        serde_json::from_slice(&input_bytes).map_err(|e| e.to_string())?;
    if let Some(dir) = assets {
        apply_asset_hashes(&mut manifest, dir, tag);
    }

    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let json_path = out.join("manifest.json");
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(&json_path, &bytes).map_err(|e| e.to_string())?;

    let signature = sign(&bytes, key_hex)?;
    fs::write(out.join("manifest.json.sig"), hex::encode(signature)).map_err(|e| e.to_string())?;

    let mut sums = collect_sha512sums(&manifest, assets);
    sums.push((sha512_hex(&bytes), "manifest.json".into()));
    sums.sort_by(|a, b| a.1.cmp(&b.1));
    sums.dedup_by(|a, b| a.1 == b.1);
    let mut sums_text = String::new();
    for (hash, name) in &sums {
        sums_text.push_str(&format!("{hash}  {name}\n"));
    }
    fs::write(out.join("SHA512SUMS.txt"), sums_text).map_err(|e| e.to_string())?;

    Ok(WriteResult {
        json_path,
        used_test_key,
        tag: tag.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_source::load_local_manifest;
    use crate::manifest::TEST_MANIFEST_PUBLIC_KEY_HEX;
    use tempfile::tempdir;

    #[test]
    fn from_folder_writes_test_signed_manifest() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("arnold-snowfall-1.3.3.jar"), b"snow-bytes").unwrap();
        fs::write(
            dir.join("arnold-snow-client.properties"),
            "quality=normal\nfpsTarget=60\nadaptiveFps=true\n",
        )
        .unwrap();
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/manifest.v3.json");
        let result = write_from_folder(&input, dir, DEFAULT_TAG).unwrap();
        assert!(result.used_test_key);
        assert!(dir.join("manifest.json").is_file());
        assert!(dir.join("manifest.json.sig").is_file());
        let sums = fs::read_to_string(dir.join("SHA512SUMS.txt")).unwrap();
        assert!(sums.contains("arnold-snowfall-1.3.3.jar"), "{sums}");
        let loaded = load_local_manifest(dir, TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap();
        assert_eq!(loaded.source, "local");
        let snow = loaded
            .manifest
            .components
            .iter()
            .find(|c| c.id == "snow")
            .unwrap();
        assert_eq!(snow.files[0].size, 10);
        assert!(snow.files[0].url.starts_with("file:"));
    }

    #[test]
    fn test_key_flag_does_not_use_an_env_signing_key() {
        let env_key = "cd".repeat(32);
        let choice = resolve_signing_key(None, Some(&env_key), true).unwrap();
        assert_ne!(choice, env_key);
        let signature = sign(b"local-source", &choice).unwrap();
        crate::manifest::verify_signature(
            b"local-source",
            &signature,
            crate::manifest::TEST_MANIFEST_PUBLIC_KEY_HEX,
        )
        .unwrap();
        match resolve_signing_key(None, Some(&choice), false) {
            Err(err) => {
                assert!(err.contains("test key"), "{err}");
                assert!(!err.contains(&choice));
            }
            Ok(_) => panic!("public signing accepted the test key"),
        }
        let explicit = "11".repeat(32);
        assert_eq!(
            resolve_signing_key(Some(&explicit), Some(&choice), true).unwrap(),
            explicit
        );
    }

    #[test]
    fn verify_release_assets_checks_sha512_and_rejects_a_mismatch() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path();
        let bytes = b"arnold-jar-bytes";
        fs::write(dir.join("arnold-ships-client-0.10.3.jar"), bytes).unwrap();
        let hash = sha512_hex(bytes);
        let tag = DEFAULT_TAG;
        let manifest = format!(
            r#"{{"components":[{{"files":[
                {{"file":"arnold-ships-client-0.10.3.jar","url":"https://github.com/Spottie97/arnold-client-mod-installer/releases/download/{tag}/arnold-ships-client-0.10.3.jar","sha512":"{hash}","size":{size}}},
                {{"file":"sodium.jar","url":"https://cdn.modrinth.com/data/x/sodium.jar","sha512":"{hash}","size":1}}
            ]}}]}}"#,
            size = bytes.len()
        );
        assert_eq!(
            verify_release_assets(manifest.as_bytes(), dir, tag).unwrap(),
            1
        );
        let bad = manifest.replace(&hash, &"ab".repeat(64));
        let err = verify_release_assets(bad.as_bytes(), dir, tag).unwrap_err();
        assert!(err.contains("sha512 mismatch"), "{err}");
        let missing = manifest.replace("arnold-ships-client-0.10.3.jar", "arnold-missing.jar");
        let err = verify_release_assets(missing.as_bytes(), dir, tag).unwrap_err();
        assert!(
            err.contains("missing") || err.contains("url is not"),
            "{err}"
        );
    }

    #[test]
    fn verify_release_assets_rejects_a_jar_hosted_on_another_tag() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path();
        let bytes = b"arnold-jar-bytes";
        fs::write(dir.join("arnold-ships-client-0.10.3.jar"), bytes).unwrap();
        let hash = sha512_hex(bytes);
        let manifest = format!(
            r#"{{"components":[{{"files":[
                {{"file":"arnold-ships-client-0.10.3.jar","url":"https://github.com/Spottie97/arnold-client-mod-installer/releases/download/mods-2026.11.2/arnold-ships-client-0.10.3.jar","sha512":"{hash}","size":{size}}}
            ]}}]}}"#,
            size = bytes.len()
        );
        let err = verify_release_assets(manifest.as_bytes(), dir, "mods-2026.11.4").unwrap_err();
        assert!(err.contains("mods-2026.11.4"), "{err}");
        assert!(err.contains("url is not"), "{err}");
    }
}
