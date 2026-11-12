#!/usr/bin/env python3
"""Parse a Tauri latest.json feed. Does not print signatures in full."""
import argparse, json, sys
from urllib.parse import urlparse

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("path")
    ap.add_argument("--expect-version", default="")
    args = ap.parse_args()
    data = json.loads(open(args.path, encoding="utf-8").read())
    version = data.get("version") or data.get("name")
    platforms = data.get("platforms") or {}
    print(f"version={version}")
    print(f"platforms={', '.join(sorted(platforms)) or '(none)'}")
    if args.expect_version and str(version).lstrip("v") != args.expect_version.lstrip("v"):
        print(f"FAIL: expected version {args.expect_version}", file=sys.stderr)
        return 1
    if not platforms:
        print("FAIL: no platforms in latest.json", file=sys.stderr)
        return 1
    ok = True
    for name, info in platforms.items():
        url = (info or {}).get("url") or ""
        sig = (info or {}).get("signature") or ""
        host = urlparse(url).netloc
        print(f"  {name}: url_host={host} has_signature={bool(sig)} sig_len={len(sig)}")
        if not url.startswith("https://"):
            print(f"FAIL: {name} url is not https", file=sys.stderr)
            ok = False
        if not sig:
            print(f"FAIL: {name} missing signature", file=sys.stderr)
            ok = False
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
