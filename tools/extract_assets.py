"""Extract embedded Tauri frontend assets from a Windows Tauri exe.

Tauri codegen stores assets as a phf map of (&str key, &[u8] brotli data).
On x64 each entry is laid out as: key_ptr, key_len, data_ptr, data_len (8 bytes each).
We locate every asset-looking path string, find the pointer that references it,
then read the adjacent data slice and brotli-decompress it.

Usage: python -I extract_assets.py <exe> <out_dir>
"""
import os
import re
import struct
import sys

import brotli
import pefile

exe, out_dir = sys.argv[1], sys.argv[2]
pe = pefile.PE(exe, fast_load=True)
data = pe.__data__[:]
base = pe.OPTIONAL_HEADER.ImageBase


def off_to_va(off):
    for s in pe.sections:
        if s.PointerToRawData <= off < s.PointerToRawData + s.SizeOfRawData:
            return base + s.VirtualAddress + (off - s.PointerToRawData)
    return None


def va_to_off(va):
    rva = va - base
    for s in pe.sections:
        if s.VirtualAddress <= rva < s.VirtualAddress + max(s.Misc_VirtualSize, s.SizeOfRawData):
            return s.PointerToRawData + (rva - s.VirtualAddress)
    return None


key_re = re.compile(rb"/[A-Za-z0-9_\-./@]{1,120}\.(?:html|js|mjs|css|json|png|jpg|jpeg|webp|gif|svg|ico|woff2?|ttf|otf|mp3|wav|ogg|mp4|webm|atlas|skel|txt|wasm|map)")

# Rust string literals are not NUL-terminated and are often concatenated, so a
# regex match may span several keys. Treat every '/' inside a match as a possible
# key start; the key length comes from the pointer's adjacent len field.
starts = set()
for m in key_re.finditer(data):
    for i in range(m.start(), m.end()):
        if data[i] == 0x2F:
            starts.add(i)

# Index all 8-byte aligned qwords once for fast pointer lookup.
ptr_index = {}
for off in range(0, len(data) - 32, 8):
    v = struct.unpack_from("<Q", data, off)[0]
    if base <= v < base + pe.OPTIONAL_HEADER.SizeOfImage:
        ptr_index.setdefault(v, []).append(off)

full_key = re.compile(key_re.pattern + rb"\Z")
found = {}
for s in starts:
    va = off_to_va(s)
    if va is None or va not in ptr_index:
        continue
    for poff in ptr_index[va]:
        klen = struct.unpack_from("<Q", data, poff + 8)[0]
        if not 2 <= klen <= 200 or not full_key.match(data[s:s + klen]):
            continue
        e = s + klen
        dptr, dlen = struct.unpack_from("<QQ", data, poff + 16)
        doff = va_to_off(dptr)
        if doff is None or dlen == 0 or dlen > 200_000_000:
            continue
        raw = data[doff:doff + dlen]
        try:
            content = brotli.decompress(raw)
        except brotli.error:
            content = raw  # some builds store uncompressed
        key = data[s:e].decode()
        found[key] = content

os.makedirs(out_dir, exist_ok=True)
for key, content in sorted(found.items()):
    path = os.path.join(out_dir, key.lstrip("/").replace("/", os.sep))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(content)
print(f"extracted {len(found)} assets, {sum(map(len, found.values()))} bytes")
for k in sorted(found)[:15]:
    print(" ", k, len(found[k]))
