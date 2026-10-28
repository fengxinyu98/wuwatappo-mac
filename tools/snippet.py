"""Print text between two offsets of a file (for reading minified code).

Usage: python -I snippet.py <file> <start> <end>
"""
import sys

sys.stdout.reconfigure(encoding="utf-8")
path, start, end = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
print(open(path, encoding="utf-8", errors="replace").read()[start:end])
