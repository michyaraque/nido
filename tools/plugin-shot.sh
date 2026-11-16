#!/usr/bin/env bash
# Screenshot of a plugin page with Nido's real CSS, without running Nido or signing in.
#
#   NIDO_PLUGIN_KEY=k NIDO_PLUGIN_ADDR=127.0.0.1:8199 NIDO_PLUGIN_DB=/tmp/test.db target/debug/nido-<name> &
#   tools/plugin-shot.sh <id> <out.png> ["query"] [width] [light|dark] ["#dialog-id"]
#
# The page is fetched with the X-Nido-* headers Nido would send, wrapped in Nido's stylesheets and
# rendered by headless Chrome. Images the plugin serves (behind its key) are saved next to the shot.
# Headless Chrome will not make a window narrower than ~500 px, so phone widths go in an iframe.
set -euo pipefail

id=$1 out=$2 query=${3:-} width=${4:-390} scheme=${5:-light} hash=${6:-}
url=${PLUGIN_URL:-http://127.0.0.1:8199}
key=${NIDO_PLUGIN_KEY:-k}
people=${NIDO_PEOPLE:-1=Ana=%232f6bff=0,2=Leo=%23e1166b=0,3=Sara=%237c3aed=0}
chrome=${CHROME:-"/c/Program Files/Google/Chrome/Application/chrome.exe"}
root=$(cd "$(dirname "$0")/.." && { pwd -W 2>/dev/null || pwd; })
dir=$(cd "$(dirname "$out")" && { pwd -W 2>/dev/null || pwd; })
name=$(basename "$out" .png)
H=(-H "X-Nido-Key: $key" -H "X-Nido-User: 1" -H "X-Nido-Name: Ana" -H "X-Nido-People: $people")

body=$(curl -sf "${H[@]}" "$url/page?$query")
for src in $(grep -o "/plugins/$id/[^\"' )]*\.\(webp\|png\|jpg\|svg\)[^\"' )]*\|/plugins/$id/[^\"' )]*/photo?v=[0-9]*" <<<"$body" | sort -u); do
  file="$name-$(printf '%s' "$src" | tr -c 'A-Za-z0-9' '_')"
  curl -sf "${H[@]}" "$url${src#/plugins/$id}" -o "$dir/$file" || true
  body=${body//"$src"/"file:///$dir/$file"}
done

{
  echo "<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width'>"
  for c in tokens base components layout pages; do echo "<link rel=stylesheet href='file:///$root/assets/css/$c.css'>"; done
  echo "<style>@font-face{font-family:Inter;src:url(file:///$root/assets/fonts/inter.woff2)}@font-face{font-family:Bricolage;src:url(file:///$root/assets/fonts/bricolage.woff2)}*{animation:none!important}</style>"
  echo "</head><body><main class=content>$body</main></body></html>"
} >"$dir/$name.html"

target="$dir/$name.html$hash" size="$width,1500"
if [ "$width" -lt 500 ]; then
  echo "<!doctype html><body style='margin:0'><iframe src='file:///$dir/$name.html$hash' style='width:${width}px;height:2400px;border:0;display:block'></iframe></body>" >"$dir/$name-frame.html"
  target="$dir/$name-frame.html" size="$width,2400"
fi
dark=1; [ "$scheme" = dark ] && dark=0
"$chrome" --headless=new --disable-gpu --hide-scrollbars --allow-file-access-from-files \
  --force-prefers-color-scheme="$scheme" --blink-settings=preferredColorScheme=$dark \
  --window-size="$size" --screenshot="$dir/$name.png" "file:///$target" 2>/dev/null
echo "$dir/$name.png"
