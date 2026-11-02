fn main() {
    println!("cargo:rerun-if-env-changed=ARNOLD_MANIFEST_PUBKEY");
    println!("cargo:rerun-if-changed=src/manifest.rs");

    let profile = std::env::var("PROFILE").unwrap_or_default();
    let local_source = std::env::var("CARGO_FEATURE_LOCAL_SOURCE").is_ok();
    if profile == "release" && !local_source {
        let src = include_str!("src/manifest.rs");
        let production = quoted_const(src, "PRODUCTION_MANIFEST_PUBLIC_KEY_HEX");
        let test_key = quoted_const(src, "TEST_MANIFEST_PUBLIC_KEY_HEX");
        let override_key = std::env::var("ARNOLD_MANIFEST_PUBKEY")
            .ok()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty());
        let key = override_key.unwrap_or(production);
        if public_manifest_key_rejected(&key, &test_key) {
            panic!(
                "public release build refused: the manifest public key is missing, the placeholder, or the test key. Set PRODUCTION_MANIFEST_PUBLIC_KEY_HEX or ARNOLD_MANIFEST_PUBKEY to the production public key."
            );
        }
    }

    tauri_build::build();
}

fn quoted_const(src: &str, name: &str) -> String {
    let marker = format!("const {name}: &str =");
    let Some(start) = src.find(&marker) else {
        panic!("src/manifest.rs is missing {name}");
    };
    let rest = &src[start + marker.len()..];
    let Some(open) = rest.find('"') else {
        panic!("{name} has no opening quote");
    };
    let rest = &rest[open + 1..];
    let Some(close) = rest.find('"') else {
        panic!("{name} has no closing quote");
    };
    rest[..close].trim().to_string()
}

fn public_manifest_key_rejected(key: &str, test_key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    let test_key = test_key.trim().to_ascii_lowercase();
    if key.len() != 64 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    key == test_key || key.chars().all(|c| c == '0')
}
