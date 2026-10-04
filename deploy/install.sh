#!/usr/bin/env bash
# Kamerton install for the WSL2 Ubuntu home server (naryad Н1-02).
# Idempotent: safe to re-run; a repeated run changes nothing that is
# already in place.
#
# Run from a repository checkout where `cargo build --release` has been
# executed (target/release/kamerton must exist); the script installs the
# pinned mlog v0.28.0 release binary itself, verifying the published
# SHA256 first.
set -euo pipefail

DATA_DIR=/data/kamerton
MLOG_TAG="v0.28.0"
MLOG_SHA="58eb1ebd0d326f071cf923acba869c4fec6c732b9458b676f158a4d743ec2260"
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "[1/7] data directory ${DATA_DIR}"
mkdir -p "${DATA_DIR}"
chmod 700 "${DATA_DIR}"

echo "[2/7] env file: token, chat, db, exec gates (skip if present)"
if [ ! -f "${DATA_DIR}/env" ]; then
  cat > "${DATA_DIR}/env" << 'ENV'
KAMERTON_TG_TOKEN=
KAMERTON_CHAT_ID=
KAMERTON_DB=/data/kamerton/kamerton.db
KAMERTON_TZ_OFFSET=3
METALOGOS_ALLOW_EXEC=1
METALOGOS_SERVE_ALLOW_EXEC=1
ENV
  chmod 600 "${DATA_DIR}/env"
  echo "      -> put KAMERTON_TG_TOKEN and KAMERTON_CHAT_ID into ${DATA_DIR}/env"
else
  echo "      -> exists, left untouched"
fi

echo "[3/7] kamerton binary from the checkout"
if [ -f "${REPO_DIR}/target/release/kamerton" ]; then
  install -m 0755 "${REPO_DIR}/target/release/kamerton" /usr/local/bin/kamerton
else
  echo "      -> target/release/kamerton missing; run: cargo build --release" >&2
  exit 1
fi

echo "[4/7] mlog ${MLOG_TAG} (SHA256-verified release asset)"
tmp="$(mktemp -d)"
base="https://github.com/ShkodnikAI/Metalogos-/releases/download/${MLOG_TAG}"
curl -fsSL -o "${tmp}/mlog" "${base}/mlog-linux-x86_64"
echo "${MLOG_SHA}  ${tmp}/mlog" | sha256sum -c - >/dev/null
chmod +x "${tmp}/mlog"
install -m 0755 "${tmp}/mlog" /usr/local/bin/mlog
rm -rf "${tmp}"
mlog --version

echo "[5/7] app.mlog, backup script, arm seed"
install -m 0644 "${REPO_DIR}/app.mlog" "${DATA_DIR}/app.mlog"
install -m 0755 "${REPO_DIR}/deploy/backup.sh" "${DATA_DIR}/backup.sh"
if [ -f "${DATA_DIR}/env" ]; then
  set -a; . "${DATA_DIR}/env"; set +a
  /usr/local/bin/kamerton seed || echo "      -> seed skipped (env not filled yet)"
fi

echo "[6/7] systemd units + wsl systemd bootstrap"
install -m 644 "${REPO_DIR}/deploy/kamerton.service" /etc/systemd/system/kamerton.service
install -m 644 "${REPO_DIR}/deploy/kamerton-poll.service" /etc/systemd/system/kamerton-poll.service
if ! grep -q '^systemd=true' /etc/wsl.conf 2>/dev/null; then
  printf '[boot]\nsystemd=true\n' >> /etc/wsl.conf
  echo "      -> systemd enabled in /etc/wsl.conf (applies after 'wsl --shutdown' from Windows)"
fi
systemctl daemon-reload

echo "[7/7] enable and start"
systemctl enable kamerton.service kamerton-poll.service
systemctl restart kamerton.service kamerton-poll.service
systemctl --no-pager --lines=5 status kamerton.service kamerton-poll.service || true

echo "Done. After a forced Windows reboot both services must come up on"
echo "their own; verify with:"
echo "  systemctl is-active kamerton.service kamerton-poll.service"
echo "The serve banner must print the exec-gate state (acceptance check)."
