"""Print source context around given identifiers in the frontend JS.

Usage: python -I context.py <dist_dir> <radius> <name> [<name> ...]
"""
import os
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
dist, radius, names = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
for root, _, files in os.walk(dist):
    for fn in sorted(files):
        if not fn.endswith(".js"):
            continue
        text = open(os.path.join(root, fn), encoding="utf-8", errors="replace").read()
        for name in names:
            for m in re.finditer(re.escape(name), text):
                s = max(0, m.start() - radius)
                print(f"--- {fn} [{name}] @{m.start()}")
                print(text[s:m.end() + radius])
