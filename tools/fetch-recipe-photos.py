"""Photos for the catalog of the Recetas extra (plugins/recipes/catalog) from Wikimedia Commons.

For each recipe: the lead image of its Spanish Wikipedia article (or a hand-picked Commons file in
PICK), only if its licence is free (CC BY, CC BY-SA, CC0, public domain). Saved as a small WebP
(400x300, ~14 KB) in plugins/recipes/photos/<slug>.webp, credited in photos/CREDITS.tsv
(slug, author, licence, file page), which the recipe page shows under the photo.

Re-run any time; existing photos are kept (delete one to fetch it again). Usage:
    python tools/fetch-recipe-photos.py            # fetch what is missing
    python tools/fetch-recipe-photos.py --sheet    # also write photos/sheet.html to review them
"""
import html, io, json, urllib.error, pathlib, re, sys, time, unicodedata, urllib.parse, urllib.request
from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parent.parent / "plugins" / "recipes"
OUT = ROOT / "photos"
UA = {"User-Agent": "NidoRecipePhotos/1.0 (self-hosted home app; recipe thumbnails)"}
FREE = re.compile(r"^(CC BY(-SA)?( \d\.\d)?|CC0|Public domain|PD)", re.I)
W, H, Q = 400, 300, 55

# Recipe name -> Commons file, when the article image is missing or not the dish.
PICK = {}
# Recipes better left with their illustration than with a wrong photo (reviewed by eye on 2026-11-14:
# the article image was a town, a flag, a raw ingredient or another dish).
SKIP = {
    "Albóndigas en salsa",
    "Alcachofas con jamón",
    "Alubias rojas con chorizo",
    "Berenjenas rellenas",
    "Bizcocho de yogur",
    "Boquerones fritos",
    "Canelones de Sant Esteve",
    "Chipirones en su tinta",
    "Chocolate a la taza",
    "Coliflor gratinada",
    "Crema de calabacín",
    "Crema de calabaza",
    "Dorada a la sal",
    "Empanada gallega de atún",
    "Ensalada de tomate y ventresca",
    "Estofado de ternera",
    "Filetes rusos",
    "Judías verdes con patatas",
    "Limonada casera",
    "Lomo adobado",
    "Lubina al horno",
    "Magdalenas caseras",
    "Paella de marisco",
    "Pochas a la navarra",
    "Pollo asado con patatas",
    "Salmón al horno",
    "Solomillo al whisky",
    "Sopa de fideos",
    "Tarta de la abuela",
    "Torrijas",
    "Tostada de aguacate y huevo",
    "Verduras asadas al horno",
}


def slug(name):
    s = unicodedata.normalize("NFD", name.lower())
    s = "".join(c for c in s if unicodedata.category(c) != "Mn")
    return "-".join(re.sub(r"[^a-z0-9]+", " ", s).split())


def get(url, tries=6):
    """GET with patience: Wikimedia answers 429 when asked too fast; wait as long as it says."""
    for i in range(tries):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=60) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            if e.code != 429 or i == tries - 1:
                raise
            time.sleep(int(e.headers.get("Retry-After") or 0) or 5 * (i + 1))


def api(host, **params):
    return json.loads(get(f"https://{host}/w/api.php?" + urllib.parse.urlencode({"format": "json", "formatversion": 2, **params})))


def lead_image(name):
    """File name of the lead image of the es.wikipedia article named like the dish (or the best search hit)."""
    for params in ({"titles": name, "redirects": 1}, {"generator": "search", "gsrsearch": name, "gsrlimit": 1}):
        pages = api("es.wikipedia.org", action="query", prop="pageimages", piprop="name", **params).get("query", {}).get("pages", [])
        for p in pages:
            if p.get("pageimage"):
                return p["pageimage"]
    return None


def text(h):
    return html.unescape(re.sub(r"<[^>]+>", "", h or "")).strip()


def info(file):
    pages = api("commons.wikimedia.org", action="query", prop="imageinfo", iiprop="url|extmetadata", iiurlwidth=500, titles="File:" + file)
    page = pages["query"]["pages"][0]
    if "imageinfo" not in page:
        return None
    ii = page["imageinfo"][0]
    meta = ii.get("extmetadata", {})
    return {
        "thumb": ii.get("thumburl") or ii["url"],
        "page": ii.get("descriptionurl", ""),
        "author": text(meta.get("Artist", {}).get("value")) or "Wikimedia Commons",
        "license": text(meta.get("LicenseShortName", {}).get("value")),
    }


def webp(data):
    img = Image.open(io.BytesIO(data)).convert("RGB")
    w, h = img.size
    if w / h > W / H:  # crop to 4:3 around the centre
        nw = int(h * W / H)
        img = img.crop(((w - nw) // 2, 0, (w - nw) // 2 + nw, h))
    else:
        nh = int(w * H / W)
        img = img.crop((0, (h - nh) // 2, w, (h - nh) // 2 + nh))
    out = io.BytesIO()
    img.resize((W, H), Image.LANCZOS).save(out, "WEBP", quality=Q, method=6)
    return out.getvalue()


def names():
    for f in sorted((ROOT / "catalog").glob("*.txt")):
        for line in f.read_text(encoding="utf-8").splitlines():
            if line.startswith("# "):
                yield line[2:].strip()


def main():
    OUT.mkdir(exist_ok=True)
    credits_file = OUT / "CREDITS.tsv"
    credits = {}
    if credits_file.exists():
        for line in credits_file.read_text(encoding="utf-8").splitlines():
            parts = line.split("\t")
            if len(parts) == 4:
                credits[parts[0]] = parts
    missing = []

    def save():
        kept = {k: v for k, v in credits.items() if (OUT / f"{k}.webp").exists()}
        credits_file.write_text("".join("\t".join(v) + "\n" for _, v in sorted(kept.items())), encoding="utf-8", newline="\n")
        return kept

    for name in names():
        s = slug(name)
        have = (OUT / f"{s}.webp").exists()
        if name in SKIP or (have and s in credits):
            continue
        try:
            file = PICK.get(name) or lead_image(name)
            meta = file and info(file)
            if not meta or not FREE.match(meta["license"]):
                missing.append(f"{name} ({meta['license'] if meta else 'sin foto'})")
                continue
            if not have:  # a photo saved earlier only needs its credit
                (OUT / f"{s}.webp").write_bytes(webp(get(meta["thumb"])))
            credits[s] = [s, meta["author"].replace("\t", " ").replace("\n", " ")[:120], meta["license"], meta["page"]]
            save()  # after every photo: an interrupted run loses nothing
            print(f"ok   {name}  <- {file}")
        except Exception as e:  # one bad recipe must not stop the rest
            missing.append(f"{name} ({e})")
        time.sleep(1)
    credits = save()
    size = sum(p.stat().st_size for p in OUT.glob("*.webp"))
    print(f"\n{len(credits)} photos, {size // 1024} KB")
    if missing:
        print("Sin foto:\n  " + "\n  ".join(missing))
    if "--sheet" in sys.argv:
        cards = "".join(f'<figure><img src="{s}.webp"><figcaption>{s}</figcaption></figure>' for s in sorted(credits))
        (OUT / "sheet.html").write_text(
            f"<!doctype html><meta charset=utf-8><style>body{{margin:8px;font:11px sans-serif;display:grid;grid-template-columns:repeat(8,1fr);gap:6px}}"
            f"figure{{margin:0}}img{{width:100%;display:block}}</style>{cards}",
            encoding="utf-8",
        )


main()
