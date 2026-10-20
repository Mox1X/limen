#!/usr/bin/env python3
"""Render ASCII snapshots using the exported font atlas; Python stdlib only.

python3 examples/preview_ascii.py docs/palettes/font shape brightness-tonal hybrid-structural
"""
import argparse
import struct
import zlib
from pathlib import Path


def chunk(kind, data):
    return (struct.pack(">I", len(data)) + kind + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("snapshots", nargs="+")
    parser.add_argument("--columns", type=int, default=120)
    args = parser.parse_args()
    info = dict(line.split("=", 1) for line in (args.directory / "atlas-info.txt").read_text().splitlines())
    cell_w, cell_h = map(int, info["cell"].split("x"))
    magic, dimensions, maximum, pixels = (args.directory / "atlas.pgm").read_bytes().split(b"\n", 3)
    atlas_w, atlas_h = map(int, dimensions.split())
    assert magic == b"P5" and maximum == b"255" and len(pixels) == atlas_w * atlas_h
    for name in args.snapshots:
        lines = [name] + (args.directory / (name + ".txt")).read_text(encoding="ascii").splitlines()
        assert args.columns > 0 and all(len(line) <= args.columns for line in lines)
        width, height = args.columns * cell_w, len(lines) * cell_h
        image = bytearray(width * height)
        for row, line in enumerate(lines):
            for column, char in enumerate(line):
                glyph = ord(char) - 32
                assert 0 <= glyph < 95
                left, top = glyph % 16 * (cell_w + 1), glyph // 16 * (cell_h + 1)
                for y in range(cell_h):
                    start = (row * cell_h + y) * width + column * cell_w
                    source = (top + y) * atlas_w + left
                    image[start:start + cell_w] = pixels[source:source + cell_w]
        raw = b"".join(b"\0" + image[y * width:(y + 1) * width] for y in range(height))
        png = (b"\x89PNG\r\n\x1a\n"
               + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0))
               + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
        (args.directory / (name + ".png")).write_bytes(png)


if __name__ == "__main__":
    main()
