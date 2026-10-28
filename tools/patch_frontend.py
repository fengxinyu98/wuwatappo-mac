"""Apply the small frontend patches the macOS build needs. Idempotent.

On Windows the app origin is http://tauri.localhost, so protocol-relative URLs
("//host/...") resolve to http(s). On macOS the origin is tauri://localhost and
they would resolve to tauri://host/..., which breaks the login SDK loader.

Usage: python -I patch_frontend.py <dist_dir>
"""
import os
import sys

PATCHES = [
    ('"//pro-cdn-sdk.kurogame.com', '"https://pro-cdn-sdk.kurogame.com'),
]

dist = sys.argv[1]
changed = 0
for root, _, files in os.walk(dist):
    for fn in files:
        if not fn.endswith((".js", ".html", ".css")):
            continue
        path = os.path.join(root, fn)
        with open(path, encoding="utf-8") as f:
            text = f.read()
        new = text
        for old, rep in PATCHES:
            new = new.replace(old, rep)
        if new != text:
            with open(path, "w", encoding="utf-8", newline="") as f:
                f.write(new)
            changed += 1
            print("patched", os.path.relpath(path, dist))
print(f"{changed} file(s) changed")
