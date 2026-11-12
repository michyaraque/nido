"""Convert every assets/logos/* (png/jpg/ico) to <slug>.webp, max 72 px (36 px @2x), then delete the original."""
import pathlib
from PIL import Image

MAX = 72
logos = pathlib.Path(__file__).resolve().parent.parent / "assets" / "logos"
before = after = 0
for p in sorted(logos.iterdir()):
    if p.suffix == ".webp":
        continue
    before += p.stat().st_size
    with Image.open(p) as im:
        if getattr(im, "n_frames", 1) > 1 or p.suffix == ".ico":
            # ICO: pick the largest embedded size
            sizes = getattr(im, "info", {}).get("sizes") or [im.size]
            im.size = max(sizes)
        img = im.convert("RGBA")
    img.thumbnail((MAX, MAX), Image.LANCZOS)  # only ever shrinks
    out = p.with_suffix(".webp")
    img.save(out, "WEBP", quality=90, method=6)
    after += out.stat().st_size
    p.unlink()
print(f"{before // 1024} KB -> {after // 1024} KB")
