"""List every snake_case string literal passed as the first argument to a short
(minified) function, i.e. candidate Tauri invoke() calls, grouped by callee.

Usage: python -I list_invokes.py <dist_dir>
"""
import collections
import os
import re
import sys

d = os.path.join(sys.argv[1], "assets")
calls = collections.defaultdict(set)
call_re = re.compile(r'([A-Za-z_$][\w$]{0,3})\(\s*"([a-z][a-z0-9]*(?:_[a-z0-9]+)+)"')
var_re = re.compile(r'const ([\w$]+)="([a-z][a-z0-9]*(?:_[a-z0-9]+)+)";([\w$]+)\(\1')
for f in sorted(os.listdir(d)):
    if not f.endswith(".js"):
        continue
    t = open(os.path.join(d, f), encoding="utf-8", errors="replace").read()
    for m in call_re.finditer(t):
        calls[m.group(2)].add(f"{m.group(1)}@{f}")
    for m in var_re.finditer(t):
        calls[m.group(2)].add(f"{m.group(3)}@{f}")
for k in sorted(calls):
    print(f"{k:48} {' '.join(sorted(calls[k]))}")
