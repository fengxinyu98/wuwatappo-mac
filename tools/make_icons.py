"""Generate Tauri bundle icons from the app icon extracted from the Windows build.

On a Mac you can instead run `cargo tauri icon ../dist/app-icon.png`.

Usage: python -I make_icons.py <app-icon.png> <out_dir>
"""
import os
import sys

from PIL import Image

src, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
img = Image.open(src).convert("RGBA")
side = max(img.size)
canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
canvas.paste(img, ((side - img.width) // 2, (side - img.height) // 2))

for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
    canvas.resize((size, size), Image.LANCZOS).save(os.path.join(out, name))
canvas.resize((1024, 1024), Image.LANCZOS).save(
    os.path.join(out, "icon.icns"), sizes=[(16, 16), (32, 32), (64, 64), (128, 128), (256, 256), (512, 512), (1024, 1024)]
)
canvas.resize((256, 256), Image.LANCZOS).save(os.path.join(out, "icon.ico"), sizes=[(16, 16), (32, 32), (48, 48), (256, 256)])
print("icons written to", out)
