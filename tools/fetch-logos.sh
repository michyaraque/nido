#!/bin/sh
# Descarga el icono de cada marca de assets/merchants.tsv y lo deja en assets/logos/<slug>.webp.
# Se ejecuta a mano al añadir marcas; los iconos se suben al repo y van dentro del binario
# (la Pi nunca sale a internet para pintar logos).
#   sh tools/fetch-logos.sh          # solo las que faltan
#   sh tools/fetch-logos.sh --all    # todas otra vez
set -eu
cd "$(dirname "$0")/.."
mkdir -p assets/logos
tmp=$(mktemp)

png_width() { od -An -tu1 -j16 -N4 "$1" | awk '{print $1*16777216+$2*65536+$3*256+$4}'; }

grep -v '^#' assets/merchants.tsv | while IFS='|' read -r slug name domain cat kw; do
  [ -n "$slug" ] && [ -n "$domain" ] || continue
  [ "${1:-}" != --all ] && ls "assets/logos/$slug".* >/dev/null 2>&1 && continue
  got=""
  # 1) Google s2 (PNG). Por debajo de 32 px se ve borroso.
  for d in "$domain" "www.$domain"; do
    if curl -fsSL -o "$tmp" "https://www.google.com/s2/favicons?domain=$d&sz=128" 2>/dev/null \
       && [ "$(png_width "$tmp")" -ge 32 ]; then
      cp "$tmp" "assets/logos/$slug.png"; got="google $(png_width "$tmp")px"; break
    fi
  done
  # 2) DuckDuckGo (ICO multi-resolución). Su "no encontrado" es un PNG de 1478 bytes con 404.
  if [ -z "$got" ] && curl -fsSL -o "$tmp" "https://icons.duckduckgo.com/ip3/$domain.ico" 2>/dev/null \
     && [ "$(wc -c < "$tmp")" -gt 2000 ]; then
    cp "$tmp" "assets/logos/$slug.ico"; got="ddg ico"
  fi
  if [ -n "$got" ]; then echo "ok   $slug ($got)"; else echo "none $slug (se usará la inicial)"; fi
done
rm -f "$tmp"
python -I tools/to_webp.py   # todo a WebP de 72 px (Pillow)
