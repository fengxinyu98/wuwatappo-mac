"""Resolve a minified imported binding back to its definition in the main chunk.

Usage: python -I resolve_import.py <assets_dir> <consumer.js> <local_name> [...]
"""
import os
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
assets, consumer, names = sys.argv[1], sys.argv[2], sys.argv[3:]
main_js = next(f for f in os.listdir(assets) if f.startswith("index-D8GJxFG7"))
main = open(os.path.join(assets, main_js), encoding="utf-8").read()
cons = open(os.path.join(assets, consumer), encoding="utf-8").read()

imp = re.search(r'import\{([^}]*)\}from"\./index-D8GJxFG7\.js"', cons).group(1)
alias = {}  # local -> exported
for part in imp.split(","):
    bits = part.split(" as ")
    alias[bits[-1].strip()] = bits[0].strip()

exp = re.findall(r"export\{([^}]*)\}", main)[-1]
exported = {}  # exported -> internal
for part in exp.split(","):
    bits = part.split(" as ")
    exported[bits[-1].strip()] = bits[0].strip()

for n in names:
    ex = alias.get(n)
    internal = exported.get(ex) if ex else None
    print(f"== {n} -> export {ex} -> {internal}")
    if internal:
        pat = re.compile(r"(?:const|let|var|function)\s+" + re.escape(internal) + r"\b|[,{;]" + re.escape(internal) + r"=")
        for m in list(pat.finditer(main))[:3]:
            print(main[m.start():m.start() + 400])
            print("..")
