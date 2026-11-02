use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha512};

/// TEST-ONLY key. Production signing uses MANIFEST_SIGNING_KEY (CI secret).
pub const TEST_PRIVATE_KEY_HEX: &str =
    "48ab0651056657627942de64e86d5435d7391e1ecd922679081c2fbadc376c98";
pub const TEST_PUBLIC_KEY_HEX: &str =
    "b44547ef0ceb7e4bac652ab9b73b46b2ff3d86d27196a479b674be29e87fdbef";
pub const DEFAULT_TAG: &str = "mods-2026.11.1";

fn sha512_hex(data: &[u8]) -> String {
    hex::encode(Sha512::digest(data))
}

pub fn usage_line() -> String {
    format!(
        "release-manifest --in <manifest.json> --out <dir> [--assets <dir>] [--from-folder <dir>] [--tag {DEFAULT_TAG}] [--key-hex HEX] [--test-key] [--sign-as-is]"
    )
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
}
