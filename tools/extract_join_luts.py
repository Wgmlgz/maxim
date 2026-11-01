#!/usr/bin/env python3
"""Reproduce assets/join_luts.bin from the supplied IVONA engine DLL."""

import hashlib
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "extracted/app/x64/tts_engine.dll"
OUTPUT = ROOT / "assets/join_luts.bin"
EXPECTED_SHA256 = "116cc3414019ce93015558da68901440e0a5596f32e326ac6b81fba56ed8bb09"

data = SOURCE.read_bytes()
if hashlib.sha256(data).hexdigest() != EXPECTED_SHA256:
    raise SystemExit("tts_engine.dll differs from the analyzed binary")

pe = struct.unpack_from("<I", data, 0x3C)[0]
if data[pe : pe + 4] != b"PE\0\0":
    raise SystemExit("invalid PE header")
section_count = struct.unpack_from("<H", data, pe + 6)[0]
optional_size = struct.unpack_from("<H", data, pe + 20)[0]
image_base = struct.unpack_from("<Q", data, pe + 24 + 24)[0]
sections = []
for i in range(section_count):
    pos = pe + 24 + optional_size + 40 * i
    virtual_size, virtual_address, raw_size, raw_address = struct.unpack_from(
        "<IIII", data, pos + 8
    )
    sections.append((virtual_address, virtual_size, raw_address, raw_size))


def read_i32(virtual_address: int) -> int:
    rva = virtual_address - image_base
    for start, size, raw, raw_size in sections:
        if start <= rva < start + min(size, raw_size) - 3:
            return struct.unpack_from("<i", data, raw + rva - start)[0]
    raise ValueError(f"address {virtual_address:#x} is outside mapped sections")


# Centered difference tables addressed by PTR_DAT_18012c2d8/e0/e8/f0.
# Offset 0 uses only an absolute normalized difference in [0, 50]. The other
# tables use signed differences; +/-512 covers the entire supplied corpus.
tables = [
    (0x1800D3A68, range(0, 51)),
    (0x1800D02C0, range(-512, 513)),
    (0x1800D1270, range(-512, 513)),
    (0x1800D29F0, range(-512, 513)),
]
payload = bytearray(b"MJL1")
for base, indices in tables:
    for index in indices:
        payload.extend(struct.pack("<i", read_i32(base + 4 * index)))
OUTPUT.write_bytes(payload)
print(f"wrote {OUTPUT} ({len(payload)} bytes)")
