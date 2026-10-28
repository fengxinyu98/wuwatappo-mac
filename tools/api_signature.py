"""Print a stable signature of the frontend <-> backend contract, for diffing versions.

Lists `invoke("cmd", {argKeys...})` calls with their top-level argument keys, plus
kebab-case string literals that also appear in the exe (event names).

Usage: python -I api_signature.py <exe> <dist_dir>
"""
import os
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
exe, dist = sys.argv[1], sys.argv[2]
blob = open(exe, "rb").read()
calls, events = set(), set()
call_re = re.compile(r'[\w$]{1,4}\(\s*"([a-z][a-z0-9]*(?:_[a-z0-9]+)+)"\s*(?:,\s*\{([^{}]*)\})?')
event_re = re.compile(r'"([a-z][a-z0-9]*(?:-[a-z0-9]+)+)"')
for root, _, files in os.walk(dist):
    for fn in files:
        if not fn.endswith(".js"):
            continue
        t = open(os.path.join(root, fn), encoding="utf-8", errors="replace").read()
        for m in call_re.finditer(t):
            keys = sorted({k.split(":")[0].strip() for k in (m.group(2) or "").split(",") if k.strip()})
            calls.add(f"{m.group(1)}({','.join(keys)})")
        for m in event_re.finditer(t):
            if m.group(1).encode() in blob:
                events.add(m.group(1))
for c in sorted(calls):
    print("call", c)
for e in sorted(events):
    print("event", e)
