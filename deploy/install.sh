#!/bin/sh
# Installs Nido on a Raspberry Pi (Raspberry Pi OS / Debian). Run it again to update the setup.
#   sudo sh install.sh                      # uses the "nido" binary next to this script
#   sudo sh install.sh user/repo [token]    # downloads releases from GitHub and keeps them updated
# The token is only needed for a private repository (fine-grained, read-only "Contents").
set -eu

REPO="${1:-}"
TOKEN="${2:-}"
HERE="$(cd "$(dirname "$0")" && pwd)"
[ "$(id -u)" = 0 ] || { echo "Ejecútalo con sudo"; exit 1; }

echo "· Paquetes"
apt-get update -qq
apt-get install -y -qq curl jq avahi-daemon avahi-utils caddy >/dev/null

echo "· Usuario y carpetas"
id nido >/dev/null 2>&1 || useradd --system --home-dir /var/lib/nido --shell /usr/sbin/nologin nido
mkdir -p /opt/nido /var/lib/nido/models /var/lib/nido/tickets /etc/nido
chown -R nido:nido /var/lib/nido

echo "· Lector de tickets (PaddleOCR PP-OCRv6 Tiny, ~6 MB)"
rm -f /var/lib/nido/models/text-detection.onnx /var/lib/nido/models/text-recognition.onnx
for m in pp-ocrv6_tiny_det.onnx pp-ocrv6_tiny_rec.onnx ppocrv6_tiny_dict.txt; do
  f="/var/lib/nido/models/$m"
  if [ -f "$HERE/models/$m" ]; then
    cp "$HERE/models/$m" "$f"
  elif [ ! -s "$f" ]; then
    curl -fsSL -o "$f" "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0/$m"
  fi
done
chown -R nido:nido /var/lib/nido/models

umask 077
printf 'REPO=%s\nTOKEN=%s\n' "$REPO" "$TOKEN" > /etc/nido/update.env
umask 022

cat > /opt/nido/update.sh <<'EOF'
#!/bin/sh
set -eu
. /etc/nido/update.env
[ -n "$REPO" ] || exit 0
cd /opt/nido
case "$(uname -m)" in
  aarch64) ARCH=aarch64 ;;
  armv7l)  ARCH=armv7 ;;
  *) echo "Arquitectura no soportada: $(uname -m)"; exit 1 ;;
esac
AUTH=""
[ -n "${TOKEN:-}" ] && AUTH="Authorization: Bearer $TOKEN"

REL=$(curl -fsSL -H "Accept: application/vnd.github+json" ${AUTH:+-H "$AUTH"} \
  "https://api.github.com/repos/$REPO/releases/latest")
TAG=$(printf '%s' "$REL" | jq -r .tag_name)
[ "$TAG" = "$(cat VERSION 2>/dev/null || true)" ] && exit 0

URL=$(printf '%s' "$REL" | jq -r --arg n "nido-$ARCH" '.assets[] | select(.name==$n) | .url')
[ -n "$URL" ] || { echo "La versión $TAG no incluye nido-$ARCH"; exit 1; }
curl -fsSL -H "Accept: application/octet-stream" ${AUTH:+-H "$AUTH"} -o nido.new "$URL"
chmod +x nido.new
./nido.new --version >/dev/null

[ -f nido ] && mv nido nido.old
mv nido.new nido
systemctl restart nido
sleep 3
if systemctl is-active --quiet nido; then
  echo "$TAG" > VERSION
  echo "Actualizado a $TAG"
else
  echo "$TAG no arranca: vuelvo a la versión anterior"
  [ -f nido.old ] && mv nido.old nido
  systemctl restart nido
  exit 1
fi
EOF
chmod 755 /opt/nido/update.sh

cat > /usr/local/bin/nido-user <<'EOF'
#!/bin/sh
# Usage: sudo nido-user <name>   (creates the user or changes the password)
set -eu
[ -n "${1:-}" ] || { echo "Uso: sudo nido-user <nombre>"; exit 1; }
printf "Contraseña para %s: " "$1"; stty -echo; read -r P; stty echo; echo
cd /var/lib/nido
printf '%s\n' "$P" | sudo -u nido NIDO_DB=/var/lib/nido/nido.db /opt/nido/nido user "$1"
EOF
chmod 755 /usr/local/bin/nido-user

cat > /usr/local/bin/nido-deploy <<'EOF'
#!/bin/sh
# Rolls back to the previous binary if the new one does not start.
set -eu
SRC="$(getent passwd "${SUDO_USER:?Ejecútalo con sudo}" | cut -d: -f6)/nido-install"
[ -f "$SRC/nido" ] || { echo "No hay binario en $SRC"; exit 1; }
install -m 755 "$SRC/nido" /opt/nido/nido.new
/opt/nido/nido.new --version >/dev/null
if [ -d "$SRC/models" ]; then
  install -m 644 -o nido -g nido "$SRC"/models/* /var/lib/nido/models/
fi
[ -f /opt/nido/nido ] && cp /opt/nido/nido /opt/nido/nido.old
mv /opt/nido/nido.new /opt/nido/nido
systemctl restart nido
sleep 3
if systemctl is-active --quiet nido; then
  echo "Nido $(/opt/nido/nido --version) en marcha"
else
  echo "La nueva versión no arranca: vuelvo a la anterior"
  journalctl -u nido -n 15 --no-pager || true
  [ -f /opt/nido/nido.old ] && mv /opt/nido/nido.old /opt/nido/nido
  systemctl restart nido
  exit 1
fi
EOF
chmod 755 /usr/local/bin/nido-deploy
if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ]; then
  printf '%s ALL=(root) NOPASSWD: /usr/local/bin/nido-deploy\n' "$SUDO_USER" > /etc/sudoers.d/nido-deploy
  chmod 440 /etc/sudoers.d/nido-deploy
  visudo -cf /etc/sudoers.d/nido-deploy >/dev/null || rm -f /etc/sudoers.d/nido-deploy
fi

cat > /etc/systemd/system/nido.service <<'EOF'
[Unit]
Description=Nido
After=network.target

[Service]
User=nido
WorkingDirectory=/var/lib/nido
Environment=NIDO_ADDR=127.0.0.1:8080
Environment=NIDO_DB=/var/lib/nido/nido.db
Environment=NIDO_MODELS=/var/lib/nido/models
ExecStart=/opt/nido/nido
Restart=always
RestartSec=2
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/var/lib/nido
# Reading a receipt peaks around 100 MB; the rest of the time Nido uses ~10 MB.
MemoryMax=300M
Nice=5

[Install]
WantedBy=multi-user.target
EOF

# iPhones need HTTPS to keep Nido on the phone and use it offline; the authority is installed once
# per phone from http://nido.local/nido.crt. Plain http keeps working, for the iPhone Shortcut too.
cat > /etc/caddy/Caddyfile <<'EOF'
{
	skip_install_trust
	auto_https disable_redirects
	pki {
		ca local {
			name "Nido"
			root_cn "Nido · certificado de casa"
			intermediate_cn "Nido · intermedio"
		}
	}
}

(nido) {
	handle /nido.crt {
		root * /var/lib/caddy/.local/share/caddy/pki/authorities/local
		rewrite * /root.crt
		header Content-Type application/x-x509-ca-cert
		file_server
	}
	handle {
		reverse_proxy 127.0.0.1:8080
	}
}

https://nido.local {
	tls internal
	import nido
}

http://nido.local, http:// {
	import nido
}
EOF
# A house authority made before it was called "Nido" is replaced (phones then install the new one).
CA=/var/lib/caddy/.local/share/caddy/pki/authorities/local
if [ -f "$CA/root.crt" ] && ! openssl x509 -noout -subject -in "$CA/root.crt" | grep -q Nido; then
  systemctl stop caddy
  rm -rf "$CA" /var/lib/caddy/.local/share/caddy/certificates/local
fi

cat > /etc/systemd/system/nido-mdns.service <<'EOF'
[Unit]
Description=Anuncia nido.local en la red
After=network-online.target avahi-daemon.service
Wants=network-online.target
Requires=avahi-daemon.service

[Service]
ExecStart=/bin/sh -c 'exec /usr/bin/avahi-publish -a -R nido.local "$(hostname -I | cut -d" " -f1)"'
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/nido-update.service <<'EOF'
[Unit]
Description=Actualiza Nido desde GitHub Releases
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/opt/nido/update.sh
EOF

cat > /etc/systemd/system/nido-update.timer <<'EOF'
[Unit]
Description=Busca actualizaciones de Nido cada 5 minutos

[Timer]
OnBootSec=1min
OnUnitActiveSec=5min

[Install]
WantedBy=timers.target
EOF

echo "· Programa"
if [ -f "$HERE/nido" ]; then
  install -m 755 "$HERE/nido" /opt/nido/nido.new
  /opt/nido/nido.new --version >/dev/null
  mv /opt/nido/nido.new /opt/nido/nido
fi

systemctl daemon-reload
systemctl enable --now nido-mdns.service >/dev/null
if [ -n "$REPO" ]; then
  systemctl enable --now nido-update.timer >/dev/null
  /opt/nido/update.sh || echo "Todavía no hay versiones publicadas en GitHub: se instalará sola cuando la haya."
fi
[ -x /opt/nido/nido ] || { echo "Falta el programa: copia el binario 'nido' junto a este script o indica un repositorio."; exit 1; }
systemctl enable nido.service caddy >/dev/null
systemctl restart nido.service caddy
sleep 2
systemctl is-active --quiet nido.service || { journalctl -u nido -n 20 --no-pager; exit 1; }

echo
echo "Nido está en marcha: https://nido.local"
echo "En cada iPhone, instala antes el certificado de casa (Nido → Ajustes → Usar sin conexión)."
echo "Crea los usuarios con:  sudo nido-user <nombre>"
