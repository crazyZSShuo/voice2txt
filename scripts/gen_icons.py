#!/usr/bin/env python3
"""Generate minimal placeholder PNG icons for the Tauri bundle.
Run once: python scripts/gen_icons.py
Real icons should replace these before shipping.
"""

import struct, zlib, os

def make_png(size: int, r=80, g=100, b=240, a=255) -> bytes:
    """Generate a solid-color RGBA PNG of given size."""
    def chunk(tag: bytes, data: bytes) -> bytes:
        c = struct.pack(">I", len(data)) + tag + data
        crc = zlib.crc32(tag + data) & 0xFFFFFFFF
        return c + struct.pack(">I", crc)

    # IHDR
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0)  # RGB (type 2)
    # Use RGBA type 6
    ihdr = struct.pack(">II", size, size) + bytes([8, 6, 0, 0, 0])

    # Raw pixel data: each row prefixed with filter byte 0
    row = bytes([0]) + bytes([r, g, b, a] * size)
    raw = row * size
    idat_data = zlib.compress(raw)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", ihdr)
    png += chunk(b"IDAT", idat_data)
    png += chunk(b"IEND", b"")
    return png

os.makedirs("src-tauri/icons", exist_ok=True)

sizes = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "tray.png": 22,
}

for name, sz in sizes.items():
    path = f"src-tauri/icons/{name}"
    with open(path, "wb") as f:
        f.write(make_png(sz))
    print(f"  wrote {path} ({sz}x{sz})")

# icon.ico — minimal 1-image ICO (32x32)
ico_png = make_png(32)
# ICO header + ICONDIRENTRY + PNG data
header = struct.pack("<HHH", 0, 1, 1)  # reserved, type=1(ico), count=1
entry = struct.pack("<BBBBHHII",
    32, 32,   # width, height
    0,        # color count (0 = >256)
    0,        # reserved
    1,        # planes
    32,       # bit count
    len(ico_png),
    6 + 16,   # offset = header(6) + one entry(16)
)
with open("src-tauri/icons/icon.ico", "wb") as f:
    f.write(header + entry + ico_png)
print("  wrote src-tauri/icons/icon.ico")

# icon.icns — macOS (minimal, just stores the 128px PNG)
# Not required for Windows-only, but Tauri build expects it
icns_type = b"ic07"  # 128x128 PNG type
png128 = make_png(128)
block = icns_type + struct.pack(">I", 8 + len(png128)) + png128
icns = b"icns" + struct.pack(">I", 8 + len(block)) + block
with open("src-tauri/icons/icon.icns", "wb") as f:
    f.write(icns)
print("  wrote src-tauri/icons/icon.icns")

print("\nDone. Replace these placeholder icons with real artwork before shipping.")
