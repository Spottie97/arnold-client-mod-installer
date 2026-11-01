use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha512};

/// TEST-ONLY key. Production signing uses MANIFEST_SIGNING_KEY (CI secret).
const TEST_PRIVATE_KEY_HEX: &str =
    "48ab0651056657627942de64e86d5435d7391e1ecd922679081c2fbadc376c98";
const TEST_PUBLIC_KEY_HEX: &str =
    "b44547ef0ceb7e4bac652ab9b73b46b2ff3d86d27196a479b674be29e87fdbef";
const DEFAULT_TAG: &str = "mods-2026.11.1";

fn sha512_hex(data: &[u8]) -> String {
    hex::encode(Sha512::digest(data))
}

fn usage() -> ! {
    eprintln!(
        "release-manifest --in <manifest.json> --out <dir> [--assets <dir>] [--tag {DEFAULT_TAG}] [--key-hex HEX] [--test-key] [--sign-as-is]"
    );
    std::process::exit(2);
}

fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn sign(bytes: &[u8], key_hex: &str) -> Result<[u8; 64], String> {
    let raw = hex::decode(key_hex.trim()).map_err(|e| e.to_string())?;
    let arr: [u8; 32] = raw
        .try_into()
        .map_err(|_| "signing key must be 32 bytes hex".to_string())?;
    let key = SigningKey::from_bytes(&arr);
    Ok(key.sign(bytes).to_bytes())
}

fn collect_sha512sums(
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

fn apply_asset_hashes(manifest: &mut serde_json::Value, assets: &Path, tag: &str) {
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        usage();
    }
    let input = arg_value(&args, "--in").unwrap_or("fixtures/manifest.v3.json");
    let out = PathBuf::from(arg_value(&args, "--out").unwrap_or("dist/mods"));
    let assets = arg_value(&args, "--assets").map(PathBuf::from);
    let tag = arg_value(&args, "--tag").unwrap_or(DEFAULT_TAG);
    let test_key = args.iter().any(|a| a == "--test-key");
    let sign_as_is = args.iter().any(|a| a == "--sign-as-is");
    let key_hex = arg_value(&args, "--key-hex")
        .map(ToString::to_string)
        .or_else(|| std::env::var("MANIFEST_SIGNING_KEY").ok())
        .unwrap_or_else(|| {
            if test_key {
                TEST_PRIVATE_KEY_HEX.to_string()
            } else {
                eprintln!("error: pass --key-hex, MANIFEST_SIGNING_KEY, or --test-key");
                usage();
            }
        });

    let input_bytes = fs::read(input).expect("read manifest");
    if sign_as_is {
        let signature = sign(&input_bytes, &key_hex).expect("sign");
        let sig_path = PathBuf::from(format!("{input}.sig"));
        fs::write(&sig_path, hex::encode(signature)).expect("write sig");
        if test_key || key_hex == TEST_PRIVATE_KEY_HEX {
            eprintln!("warning: signed with the TEST manifest key ({TEST_PUBLIC_KEY_HEX})");
        }
        println!("signed {} as-is → {}", input, sig_path.display());
        return;
    }

    let mut manifest: serde_json::Value =
        serde_json::from_slice(&input_bytes).expect("parse manifest");
    if let Some(dir) = &assets {
        apply_asset_hashes(&mut manifest, dir, tag);
    }

    fs::create_dir_all(&out).expect("create out dir");
    let json_path = out.join("manifest.json");
    let bytes = serde_json::to_vec_pretty(&manifest).expect("serialize");
    fs::write(&json_path, &bytes).expect("write manifest");

    let signature = sign(&bytes, &key_hex).expect("sign");
    fs::write(out.join("manifest.json.sig"), hex::encode(signature)).expect("write sig");

    let mut sums = collect_sha512sums(&manifest, assets.as_deref());
    sums.push((sha512_hex(&bytes), "manifest.json".into()));
    sums.sort_by(|a, b| a.1.cmp(&b.1));
    sums.dedup_by(|a, b| a.1 == b.1);
    let mut sums_text = String::new();
    for (hash, name) in &sums {
        sums_text.push_str(&format!("{hash}  {name}\n"));
    }
    fs::write(out.join("SHA512SUMS.txt"), sums_text).expect("write SHA512SUMS");

    if test_key || key_hex == TEST_PRIVATE_KEY_HEX {
        eprintln!("warning: signed with the TEST manifest key ({TEST_PUBLIC_KEY_HEX})");
    }
    println!("wrote {} (tag {tag})", json_path.display());
}

#[cfg(test)]
mod tests {
    // binary crate tests live with the library tests
}
