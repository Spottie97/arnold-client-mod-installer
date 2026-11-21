#!/usr/bin/env python3
"""Drop byte-identical duplicate slots from the macOS app icon.

`npx tauri icon src/assets/brand/avalon-icon.png -o src-tauri/icons` writes an
icns where ic14 (256pt@2x) is the same 512 px PNG as ic09 (512pt@1x) and ic13
(128pt@2x) is the same 256 px PNG as ic08 (256pt@1x). The bundler ships that
icns twice in the dmg (app icon + volume icon), so the copies cost ~1.1 MB
against the 10 MB release gate. macOS picks an icon representation by pixel
size, so the same pixels are still drawn at those sizes from ic09/ic08.

Only a slot whose bytes equal its 1x twin is removed. Every other entry is kept
unchanged and in order. Run again after regenerating the icons:

    python3 tools/slim-icns.py src-tauri/icons/icon.icns
"""
import struct
import sys

TWINS = {b"ic14": b"ic09", b"ic13": b"ic08"}


def read_entries(data):
    if data[:4] != b"icns" or struct.unpack(">I", data[4:8])[0] != len(data):
        raise SystemExit("not an icns file")
    entries, i = [], 8
    while i < len(data):
        kind, size = data[i : i + 4], struct.unpack(">I", data[i + 4 : i + 8])[0]
        if size < 8 or i + size > len(data):
            raise SystemExit("corrupt icns entry")
        entries.append((kind, data[i + 8 : i + size]))
        i += size
    return entries


def main(path):
    with open(path, "rb") as f:
        data = f.read()
    entries = read_entries(data)
    by_kind = dict(entries)
    kept = []
    for kind, body in entries:
        twin = TWINS.get(kind)
        if twin is not None and by_kind.get(twin) == body:
            print(f"drop {kind.decode()} ({len(body)} bytes, same as {twin.decode()})")
            continue
        kept.append((kind, body))
    body = b"".join(k + struct.pack(">I", len(b) + 8) + b for k, b in kept)
    out = b"icns" + struct.pack(">I", len(body) + 8) + body
    with open(path, "wb") as f:
        f.write(out)
    print(f"{path}: {len(data)} -> {len(out)} bytes")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: slim-icns.py path/to/icon.icns")
    main(sys.argv[1])
