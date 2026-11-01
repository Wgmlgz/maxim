"""Extract question-tone patterns from the supplied voice_ru_maxim.dll.

Usage: python3 tools/extract_tone_tables.py VOICE.DLL OUTPUT.txt
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

    def string(va: int) -> str:
        start = offset(va)
        return data[start:data.index(0, start)].decode("iso8859_5")

    lines = ["# Extracted from voice_ru_maxim.dll, SHA256 " + EXPECTED_SHA256]
    for label, start, width in [
        ("question_h", 0x18003FA30, 1),
        ("question_h", 0x18003FAA8, 2),
        ("question_h", 0x18003FAE0, 3),
    ]:
        for index in range(64):
            first = pointer(start + index * width * 8)
            if first == 0:
                break
            words = [string(pointer(start + (index * width + column) * 8))
                     for column in range(width)]
            lines.append("\t".join([label, *words]))
        else:
            raise ValueError("unterminated phrase-tone table")
    for address in (0x18003FBE8, 0x18003FBEC):
        lines.append("alternative\t" + string(address))
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    Path(sys.argv[2]).write_text(extract(Path(sys.argv[1])), encoding="utf-8")
