"""Stub C compiler / archiver for `cargo check --target *-apple-darwin` on Windows.

cargo check never links, so build scripts that compile tiny C/ObjC helpers only need
*some* output file to exist. Writes an empty file to the `-o <path>` argument (cc) or to
the first `*.a` argument (ar). Never use this for a real build.
"""
import sys

args = sys.argv[1:]
target = None
if "-o" in args:
    target = args[args.index("-o") + 1]
else:
    target = next((a for a in args if a.endswith(".a")), None)
if target:
    open(target, "ab").close()
