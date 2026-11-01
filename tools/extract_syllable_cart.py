"""Extract the Russian syllable CART from the supplied voice DLL as data.

Usage: python3 tools/extract_syllable_cart.py VOICE.DLL OUTPUT.bin
"""

import hashlib
import struct
import sys
from pathlib import Path

EXPECTED_SHA256 = "6c1bb6badc0d6aa3a2b6dbffeb721e027c00fd0eab24b021140e372bf10d5d56"
ATTRIBUTES = ["vc", "cvox", "vheight", "vfront", "vrnd", "ctype", "cplace", "csoft", "stress"]


def extract(source: Path) -> bytes:
    data = source.read_bytes()
    if hashlib.sha256(data).hexdigest() != EXPECTED_SHA256:
        raise ValueError("unexpected voice DLL digest")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    section_count = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    image_base = struct.unpack_from("<Q", data, pe + 48)[0]
    sections = []
    for i in range(section_count):
        entry = pe + 24 + optional_size + 40 * i
        sections.append(
            (
                struct.unpack_from("<I", data, entry + 12)[0],
                struct.unpack_from("<I", data, entry + 8)[0],
                struct.unpack_from("<I", data, entry + 20)[0],
            )
        )

    def offset(va: int) -> int:
        rva = va - image_base
        for start, size, raw in sections:
            if start <= rva < start + size:
                return raw + rva - start
        raise ValueError(f"VA {va:#x} outside PE sections")

    def pointer(va: int) -> int:
        return struct.unpack_from("<Q", data, offset(va))[0]

    def string(va: int) -> str:
        start = offset(va)
        return data[start : data.index(0, start)].decode("ascii")

    features = []
    for i in range(43):
        va = 0x180049A80 + i * 16
        kind = struct.unpack_from("<I", data, offset(va))[0]
        depth = data[offset(va + 4)]
        attribute = ATTRIBUTES.index(string(pointer(va + 8)))
        features.extend((kind, depth, attribute))
    values = []
    for i in range(20):
        value = string(pointer(0x180049D30 + i * 8))
        if len(value) != 1:
            raise ValueError("expected one-character CART value")
        values.append(ord(value))
    node_start = offset(0x180049DD0)
    nodes = data[node_start : node_start + 289 * 6]
    seen = set()
    todo = [0]
    while todo:
        index = todo.pop()
        if index in seen:
            continue
        if index >= 289:
            raise ValueError("CART node outside extracted range")
        seen.add(index)
        feature, value, yes, no = struct.unpack_from("<BBHH", nodes, index * 6)
        if yes or no:
            if feature >= 43 or value >= 20:
                raise ValueError("CART condition outside tables")
            todo.extend((yes, no))
    if len(seen) != 289:
        raise ValueError("CART extraction did not cover all nodes")
    return b"MSC1" + struct.pack("<HHH", 43, 20, 289) + bytes(features + values) + nodes


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    output = extract(Path(sys.argv[1]))
    Path(sys.argv[2]).write_bytes(output)
    print(f"wrote {len(output)} bytes")
