"""Extract original Russian postlex consonant tables from voice_ru_maxim.dll.

Usage: python3 tools/extract_postlex_tables.py VOICE.DLL OUTPUT.txt
"""

import hashlib
import struct
import sys
from pathlib import Path

EXPECTED_SHA256 = "6c1bb6badc0d6aa3a2b6dbffeb721e027c00fd0eab24b021140e372bf10d5d56"


def extract(path: Path) -> str:
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != EXPECTED_SHA256:
        raise ValueError("unexpected voice DLL digest")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    count = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    image_base = struct.unpack_from("<Q", data, pe + 48)[0]
    sections = [
        tuple(struct.unpack_from("<I", data, pe + 24 + optional_size + 40 * i + j)[0]
              for j in (12, 8, 20))
        for i in range(count)
    ]

    def offset(va: int) -> int:
        rva = va - image_base
        for start, size, raw in sections:
            if start <= rva < start + size:
                return raw + rva - start
        raise ValueError(f"VA {va:#x} outside PE sections")

    def pointer(va: int) -> int:
        return struct.unpack_from("<Q", data, offset(va))[0]

    def string(va: int, encoding: str = "ascii") -> str:
        start = offset(va)
        return data[start : data.index(0, start)].decode(encoding)

    devoice_keys = [string(0x18003B938)] + [string(pointer(0x18003CF78 + 8 * i)) for i in range(11)]
    devoice_values = [string(pointer(0x18003D000 + 8 * i)) for i in range(12)]
    voice_keys = [string(0x180037B78)] + [string(pointer(0x18003D008 + 8 * i)) for i in range(11)]
    voice_values = [string(pointer(0x18003CF70 + 8 * i)) for i in range(12)]
    sonorants = [string(pointer(0x18003CF10 + 8 * i)) for i in range(11)]
    exceptions = [string(pointer(0x1800F6010 + 8 * i), "iso8859_5") for i in range(11)]
    anchors = []
    for i in range(64):
        entry = pointer(0x18003D090 + 8 * i)
        if entry == 0:
            break
        anchors.append(string(entry, "iso8859_5"))
    idioms = []
    for i in range(10):
        key = 0x18003D4CC if i == 0 else pointer(0x18003D3E8 + 8 * (i - 1))
        table = pointer(0x18003D440 + 8 * i)
        for j in range(100):
            value = pointer(table + 8 * j)
            if value == 0:
                break
            idioms.append((string(key, "iso8859_5"), string(value, "iso8859_5")))
    triples = [
        tuple(string(pointer(0x18003D308 + 8 * i), "iso8859_5") for i in range(3)),
        *[
            tuple(string(pointer(0x18003D498 + 8 * (3 * j + i)), "iso8859_5") for i in range(3))
            for j in range(2)
        ],
    ]
    lines = ["# Extracted from voice_ru_maxim.dll, SHA256 " + EXPECTED_SHA256]
    lines += [f"devoice\t{a}\t{b}" for a, b in zip(devoice_keys, devoice_values)]
    lines += [f"voice\t{a}\t{b}" for a, b in zip(voice_keys, voice_values)]
    lines += [f"sonorant\t{name}" for name in sonorants]
    lines += [f"exception\t{name}" for name in exceptions]
    lines += [f"reduction_anchor\t{name}" for name in anchors]
    lines += [f"reduction_pair\t{a}\t{b}" for a, b in idioms]
    lines += [f"reduction_triple\t{a}\t{b}\t{c}" for a, b, c in triples]
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    Path(sys.argv[2]).write_text(extract(Path(sys.argv[1])), encoding="utf-8")
