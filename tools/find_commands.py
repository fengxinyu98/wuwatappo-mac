"""Cross-reference snake_case/kebab string literals in the frontend JS with the exe.

Strings that appear both in the JS and in the Rust binary are very likely custom
Tauri commands (invoke) or event names (emit/listen).

Usage: python -I find_commands.py <exe> <dist_dir>
"""
import os
import re
import sys

exe, dist = sys.argv[1], sys.argv[2]
blob = open(exe, "rb").read()

lit = re.compile(r"""["'`]([a-z][a-z0-9]*(?:[_\-:][a-z0-9]+)+)["'`]""")
hits = {}
for root, _, files in os.walk(dist):
    for fn in files:
        if not fn.endswith(".js"):
            continue
        text = open(os.path.join(root, fn), encoding="utf-8", errors="replace").read()
        for m in lit.finditer(text):
            s = m.group(1)
            if s.startswith("plugin:"):
                continue
            hits.setdefault(s, set()).add(fn)

for s in sorted(hits):
    if s.encode() in blob:
        print(f"{s:45} {','.join(sorted(hits[s]))}")
