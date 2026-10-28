"""Zip the project for copying to a Mac (forward-slash paths, +x on shell scripts).

Usage: python -I pack.py <project_dir> <out.zip>
"""
import os
import sys
import zipfile

root, out = os.path.abspath(sys.argv[1]), sys.argv[2]
skip_dirs = {os.path.join(root, "src-tauri", "target"), os.path.join(root, "src-tauri", "gen")}
base = os.path.basename(root)
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for d, dirs, files in os.walk(root):
        dirs[:] = [x for x in dirs if os.path.join(d, x) not in skip_dirs and x != "__pycache__"]
        for fn in files:
            if fn.startswith("ctx") and fn.endswith(".txt") or fn == "exports.txt":
                continue
            full = os.path.join(d, fn)
            arc = base + "/" + os.path.relpath(full, root).replace(os.sep, "/")
            info = zipfile.ZipInfo.from_file(full, arc)
            mode = 0o755 if fn.endswith(".sh") else 0o644
            info.external_attr = (0o100000 | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            with open(full, "rb") as f:
                z.writestr(info, f.read())
print("wrote", out)
