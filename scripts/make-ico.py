#!/usr/bin/env python3
"""Packs square PNG files into a Windows .ico (PNG-compressed entries).

Usage: make-ico.py out.ico icon_16.png icon_32.png ...
"""
import struct
import sys


def main() -> None:
    out, *sources = sys.argv[1:]
    images = []
    for source in sources:
        with open(source, "rb") as file:
            data = file.read()
        width, height = struct.unpack(">II", data[16:24])
        images.append((width, height, data))
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for width, height, data in images:
        # 256 is written as 0.
        entries += struct.pack(
            "<BBBBHHII", width % 256, height % 256, 0, 0, 1, 32, len(data), offset
        )
        offset += len(data)
    with open(out, "wb") as file:
        file.write(header + entries + b"".join(data for _, _, data in images))


if __name__ == "__main__":
    main()
