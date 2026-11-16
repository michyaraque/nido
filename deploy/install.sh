#!/bin/sh
# Installs Nido on a Debian-based machine (Raspberry Pi OS, Debian…). Run it again to update the setup.
#   sudo sh install.sh                      # uses the "nido" binary next to this script
#   sudo sh install.sh user/repo [token]    # downloads the latest release from GitHub
# The token is only needed for a private repository (fine-grained, read-only "Contents").
# Later versions are installed from Nido itself (Ajustes → La casa → Nido), when you decide.
set -eu

REPO="${1:-}"
TOKEN="${2:-}"
HERE="$(cd "$(dirname "$0")" && pwd)"
[ "$(id -u)" = 0 ] || { echo "Ejecútalo con sudo"; exit 1; }

echo "· Paquetes"
apt-get update -qq
apt-get install -y -qq curl jq openssl tar udisks2 avahi-daemon avahi-utils caddy >/dev/null

echo "· Usuario y carpetas"
id nido >/dev/null 2>&1 || useradd --system --home-dir /var/lib/nido --shell /usr/sbin/nologin nido
mkdir -p /opt/nido /var/lib/nido/models /var/lib/nido/tickets /var/lib/nido/plugins /var/lib/nido/update
# Plugins: Nido downloads them here from its Plugins page, so the folder is its own.
chown nido:nido /var/lib/nido/plugins
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

# Updates used to install themselves every 5 minutes; now each house decides from Ajustes.
systemctl disable --now nido-update.timer >/dev/null 2>&1 || true
rm -f /etc/systemd/system/nido-update.timer /etc/systemd/system/nido-update.service /opt/nido/update.sh /etc/nido/update.env
rmdir /etc/nido 2>/dev/null || true

# Nido downloads and checks the new version into /var/lib/nido/update and writes "request";
# this script (root) puts it in place, restarts Nido and goes back if it does not start.
cat > /opt/nido/upgrade.sh <<'EOF'
#!/bin/sh
set -u
D=/var/lib/nido/update
# Whatever touches the folder runs as nido: it is Nido's, so root never follows a link left there.
say() { printf '%s\t%s\n' "$1" "$2" | runuser -u nido -- tee -a "$D/log" >/dev/null; }
fail() { say fail "$1"; exit 1; }
[ -f "$D/request" ] || exit 0
TAG=$(runuser -u nido -- head -c 40 "$D/request" | tr -cd 'v0-9.')
say sys "Instalador del sistema: instalando $TAG en /opt/nido"
rm -f "$D/request"
{ [ -f "$D/nido" ] && [ ! -L "$D/nido" ]; } || fail "No está la versión descargada. No se ha cambiado nada."
GOT=$(runuser -u nido -- "$D/nido" --version 2>/dev/null) || fail "La versión descargada no arranca. No se ha cambiado nada."
[ "$GOT" = "$TAG" ] || fail "Se esperaba $TAG y lo descargado es $GOT. No se ha cambiado nada."
runuser -u nido -- cat "$D/nido" > /opt/nido/nido.new || fail "No se ha podido copiar a /opt/nido. No se ha cambiado nada."
chmod 755 /opt/nido/nido.new
[ -f /opt/nido/nido ] && cp /opt/nido/nido /opt/nido/nido.old
mv /opt/nido/nido.new /opt/nido/nido
say sys "Reiniciando Nido con la $TAG"
systemctl restart nido
up() { systemctl is-active --quiet nido && [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 http://127.0.0.1:8080/ping)" = 204 ]; }
n=0
while [ $n -lt 20 ] && ! up; do sleep 1; n=$((n + 1)); done
sleep 3
if up; then
  rm -f "$D/nido"
  say ok "Nido $TAG en marcha"
else
  say sys "La $TAG no arranca: volviendo a la versión anterior"
  journalctl -u nido -n 5 --no-pager -o cat 2>/dev/null | while IFS= read -r l; do say sys "  $l"; done
  [ -f /opt/nido/nido.old ] && mv /opt/nido/nido.old /opt/nido/nido
  systemctl restart nido
  fail "La $TAG no ha arrancado y Nido ha vuelto a la versión anterior."
fi
EOF
chmod 755 /opt/nido/upgrade.sh

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

# Lets Nido mount the USB disk chosen for backups (and nothing else), also after a reboot or a replug.
mkdir -p /etc/polkit-1/rules.d
cat > /etc/polkit-1/rules.d/50-nido-usb.rules <<'EOF'
polkit.addRule(function (action, subject) {
  if (subject.user == "nido" && [
        "org.freedesktop.udisks2.filesystem-mount",
        "org.freedesktop.udisks2.filesystem-mount-other-seat",
        "org.freedesktop.udisks2.filesystem-unmount-others"
      ].indexOf(action.id) >= 0) {
    return polkit.Result.YES;
  }
});
EOF

cat > /usr/local/bin/nido-restore <<'EOF'
#!/bin/sh
# Usage: sudo nido-restore <nido-AAAAMMDD-HHMMSS.tar.gz.enc>
# Replaces Nido's data with the copy; what was there before is kept in /var/lib/nido.before-restore.
set -eu
[ -f "${1:-}" ] || { echo "Uso: sudo nido-restore <copia.tar.gz.enc>"; exit 1; }
printf "Contraseña de las copias: "; stty -echo; read -r P; stty echo; echo
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
NIDO_BACKUP_PASS="$P" openssl enc -d -aes-256-cbc -pbkdf2 -iter 200000 -pass env:NIDO_BACKUP_PASS -in "$1" -out "$TMP/copia.tar.gz" 2>/dev/null \
  || { echo "Contraseña incorrecta o copia dañada."; exit 1; }
tar -xzf "$TMP/copia.tar.gz" -C "$TMP"
[ -f "$TMP/nido.db" ] || { echo "La copia no contiene nido.db."; exit 1; }
systemctl stop nido
rm -rf /var/lib/nido.before-restore
mkdir -p /var/lib/nido.before-restore
for f in nido.db nido.db-wal nido.db-shm tickets avatars; do
  [ -e "/var/lib/nido/$f" ] && mv "/var/lib/nido/$f" /var/lib/nido.before-restore/
done
mv "$TMP/nido.db" /var/lib/nido/
for d in tickets avatars; do [ -d "$TMP/$d" ] && mv "$TMP/$d" /var/lib/nido/; done
# Plugin databases: plugins/data/ in current backups, extras/ in older ones.
[ -d /var/lib/nido/plugins/data ] && mv /var/lib/nido/plugins/data /var/lib/nido.before-restore/plugins-data
mkdir -p /var/lib/nido/plugins/data
for d in "$TMP/plugins/data" "$TMP/extras"; do
  [ -d "$d" ] && find "$d" -maxdepth 1 -type f -exec mv {} /var/lib/nido/plugins/data/ \;
done
chown -R nido:nido /var/lib/nido
systemctl start nido
echo "Copia recuperada. Lo que había antes está en /var/lib/nido.before-restore."
EOF
chmod 755 /usr/local/bin/nido-restore

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
# Plugins built at home (nido-<name>): the same place the marketplace downloads them to.
install -d -o nido -g nido /var/lib/nido/plugins
chown nido:nido /var/lib/nido/plugins # Nido downloads and updates plugins here
for p in "$SRC"/plugins/nido-*; do
  [ -f "$p" ] || continue
  chmod 755 "$p" # copied from Windows it arrives without the execute bit
  "$p" --version >/dev/null && install -D -m 755 -o nido -g nido "$p" "/var/lib/nido/plugins/$(basename "$p")"
done
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
ReadWritePaths=/var/lib/nido -/media -/mnt
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
	# Pages are HTML made on this machine: compressed they travel ~8x lighter to the phone.
	encode zstd gzip
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

cat > /etc/systemd/system/nido-upgrade.service <<'EOF'
[Unit]
Description=Instala la versión de Nido pedida desde Ajustes

[Service]
Type=oneshot
ExecStart=/opt/nido/upgrade.sh
EOF

cat > /etc/systemd/system/nido-upgrade.path <<'EOF'
[Unit]
Description=Espera a que Nido pida instalar una versión nueva

[Path]
PathExists=/var/lib/nido/update/request

[Install]
WantedBy=multi-user.target
EOF

echo "· Programa"
if [ -f "$HERE/nido" ]; then
  install -m 755 "$HERE/nido" /opt/nido/nido.new
elif [ -n "$REPO" ] && [ ! -x /opt/nido/nido ]; then
  case "$(uname -m)" in
    aarch64) ARCH=aarch64 ;;
    armv7l)  ARCH=armv7 ;;
    armv6l)  ARCH=armv6 ;;
    *) echo "Arquitectura no soportada: $(uname -m)"; exit 1 ;;
  esac
  AUTH=""
  [ -n "$TOKEN" ] && AUTH="Authorization: Bearer $TOKEN"
  URL=$(curl -fsSL -H "Accept: application/vnd.github+json" ${AUTH:+-H "$AUTH"} "https://api.github.com/repos/$REPO/releases/latest" \
    | jq -r --arg n "nido-$ARCH" '.assets[] | select(.name==$n) | .url')
  [ -n "$URL" ] || { echo "La última versión de $REPO no incluye nido-$ARCH"; exit 1; }
  curl -fsSL -H "Accept: application/octet-stream" ${AUTH:+-H "$AUTH"} -o /opt/nido/nido.new "$URL"
  chmod 755 /opt/nido/nido.new
fi
if [ -f /opt/nido/nido.new ]; then
  /opt/nido/nido.new --version >/dev/null
  mv /opt/nido/nido.new /opt/nido/nido
fi

systemctl daemon-reload
systemctl enable --now nido-mdns.service >/dev/null
systemctl enable --now nido-upgrade.path >/dev/null
[ -x /opt/nido/nido ] || { echo "Falta el programa: copia el binario 'nido' junto a este script o indica un repositorio."; exit 1; }
systemctl enable nido.service caddy >/dev/null
systemctl restart nido.service caddy
sleep 2
systemctl is-active --quiet nido.service || { journalctl -u nido -n 20 --no-pager; exit 1; }

echo
echo "Nido está en marcha: https://nido.local"
echo "En cada iPhone, instala antes el certificado de casa (Nido → Ajustes → Usar sin conexión)."
echo "Crea los usuarios con:  sudo nido-user <nombre>"
