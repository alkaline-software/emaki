#!/usr/bin/env python3
"""ico.py <out.ico> <png>...  : a Windows .ico holding each PNG as it is
(Vista and later read PNG entries; every size in one file)."""
import struct, sys
out, pngs = sys.argv[1], sys.argv[2:]
entries, blobs, offset = [], [], 6 + 16 * len(pngs)
for p in pngs:
    b = open(p, "rb").read()
    w, h = struct.unpack(">II", b[16:24])
    entries.append(struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(b), offset))
    blobs.append(b)
    offset += len(b)
with open(out, "wb") as f:
    f.write(struct.pack("<HHH", 0, 1, len(pngs)))
    f.write(b"".join(entries))
    f.write(b"".join(blobs))
print("wrote", out)
