use std::path::PathBuf;
use std::process;

use arnold_client_mods_lib::release_tool::{
    arg_value, resolve_signing_key, sign, usage_line, verify_release_assets, write_signed,
    DEFAULT_TAG, TEST_PRIVATE_KEY_HEX, TEST_PUBLIC_KEY_HEX,
};

fn usage() -> ! {
    eprintln!("{}", usage_line());
    eprintln!(
        "  --from-folder DIR  hash jars/settings in DIR, write a TEST-signed manifest.json + .sig there"
    );
    process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        usage();
    }
    let from_folder = arg_value(&args, "--from-folder").map(PathBuf::from);
    let input = PathBuf::from(arg_value(&args, "--in").unwrap_or("fixtures/manifest.v3.json"));
    let out = arg_value(&args, "--out")
        .map(PathBuf::from)
        .or_else(|| from_folder.clone())
        .unwrap_or_else(|| PathBuf::from("dist/mods"));
    let assets = from_folder
        .clone()
        .or_else(|| arg_value(&args, "--assets").map(PathBuf::from));
    let tag = arg_value(&args, "--tag").unwrap_or(DEFAULT_TAG);
    let test_key = args.iter().any(|a| a == "--test-key") || from_folder.is_some();
    let sign_as_is = args.iter().any(|a| a == "--sign-as-is");
    if let Some(dir) = arg_value(&args, "--verify-assets") {
        let input_bytes = std::fs::read(&input).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        });
        match verify_release_assets(&input_bytes, std::path::Path::new(dir), tag) {
            Ok(count) => println!("verified {count} release assets"),
            Err(err) => {
                eprintln!("error: {err}");
                process::exit(1);
            }
        }
        return;
    }
    let env_key = std::env::var("MANIFEST_SIGNING_KEY").ok();
    let key_hex = resolve_signing_key(arg_value(&args, "--key-hex"), env_key.as_deref(), test_key)
        .unwrap_or_else(|err| {
            eprintln!("error: {err}");
            usage();
        });

    if sign_as_is {
        let input_bytes = std::fs::read(&input).expect("read manifest");
        let signature = sign(&input_bytes, &key_hex).expect("sign");
        let sig_path = PathBuf::from(format!("{}.sig", input.display()));
        std::fs::write(&sig_path, hex::encode(signature)).expect("write sig");
        if test_key || key_hex == TEST_PRIVATE_KEY_HEX {
            eprintln!("warning: signed with the TEST manifest key ({TEST_PUBLIC_KEY_HEX})");
        }
        println!("signed {} as-is → {}", input.display(), sig_path.display());
        return;
    }

    let result = write_signed(&input, &out, assets.as_deref(), tag, &key_hex, false)
        .unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        });
    if result.used_test_key {
        eprintln!("warning: signed with the TEST manifest key ({TEST_PUBLIC_KEY_HEX})");
    }
    println!("wrote {} (tag {tag})", result.json_path.display());
}
