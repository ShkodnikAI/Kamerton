#!/usr/bin/env bash
# Kamerton install for the WSL2 Ubuntu home server (naryad Н1-02).
# Idempotent: safe to re-run; a repeated run changes nothing that is
# already in place.
set -euo pipefail

DATA_DIR=/data/kamerton
SERVICE=/etc/systemd/system/kamerton.service

echo "[1/5] data directory ${DATA_DIR}"
mkdir -p "${DATA_DIR}"
chmod 700 "${DATA_DIR}"

echo "[2/5] env file with the token (skip if present)"
if [ ! -f "${DATA_DIR}/env" ]; then
  printf 'KAMERTON_TG_TOKEN=\n' > "${DATA_DIR}/env"
  chmod 600 "${DATA_DIR}/env"
  echo "      -> put KAMERTON_TG_TOKEN=... into ${DATA_DIR}/env"
else
  echo "      -> exists, left untouched"
fi

echo "[3/5] systemd unit"
install -m 644 "$(dirname "$0")/kamerton.service" "${SERVICE}"

echo "[4/5] systemd + wsl systemd bootstrap"
if ! grep -q '^systemd=true' /etc/wsl.conf 2>/dev/null; then
  printf '[boot]\nsystemd=true\n' >> /etc/wsl.conf
  echo "      -> systemd enabled in /etc/wsl.conf (applies after 'wsl --shutdown' from Windows)"
fi
systemctl daemon-reload

echo "[5/5] enable and start"
systemctl enable kamerton.service
systemctl restart kamerton.service
systemctl --no-pager --lines=5 status kamerton.service

echo "Done. After a forced Windows reboot the service must come up on its own;"
echo "verify with: systemctl is-active kamerton.service"
