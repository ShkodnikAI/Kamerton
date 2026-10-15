#!/usr/bin/env bash
# Kamerton home-server installer (naryad Н1-02): WSL2 Ubuntu, systemd,
# `mlog serve app.mlog`, database on /data, weekly backup to an external
# drive. Idempotent: a repeated run changes nothing that is already in
# place. Run with sudo from a repository checkout:
#
#   sudo bash deploy/install.sh
#   DRY_RUN=1 sudo -E bash deploy/install.sh   # print actions only
#
# Acceptance (self-host/README.md): after a forced Windows reboot the
# service must come up on its own; the serve banner must print the
# exec/env gate states expected by deploy/env.allowlist; the backup
# script must restore and read back the database.
set -euo pipefail

DATA_DIR=/data/kamerton
MLOG_TAG="v0.32.0"
MLOG_SHA="dd8164b747aa99cb953d3f7a29e084ba38d432df376aa2f05cfd2187e2bc59a2"
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RUN_AS="${SUDO_USER:-}"

DRY_RUN="${DRY_RUN:-0}"
step() { echo "[install] $*"; }
run()  { if [ "${DRY_RUN}" = "1" ]; then echo "    + $*"; else "$@"; fi; }

if [ "${DRY_RUN}" != "1" ] && [ "$(id -u)" != "0" ]; then
  echo "install: run with sudo inside WSL2 Ubuntu (or DRY_RUN=1 to preview)" >&2
  exit 1
fi
if [ -z "${RUN_AS}" ]; then
  echo "install: SUDO_USER is empty — cannot pick the service user" >&2
  exit 1
fi

step "1/8 data directory ${DATA_DIR}"
run mkdir -p "${DATA_DIR}"
run chmod 700 "${DATA_DIR}"
run chown "${RUN_AS}:${RUN_AS}" "${DATA_DIR}"

step "2/8 env file: token, chat, db, allowlist (skip if present)"
if [ ! -f "${DATA_DIR}/env" ]; then
  ALLOWLIST="$(grep -vE '^\s*(#|$)' "${REPO_DIR}/deploy/env.allowlist" | paste -sd, -)"
  if [ "${DRY_RUN}" = "1" ]; then
    echo "    + write ${DATA_DIR}/env (allowlist: ${ALLOWLIST})"
  else
    umask 077
    cat > "${DATA_DIR}/env" <<ENV
KAMERTON_TG_TOKEN=
KAMERTON_CHAT_ID=
KAMERTON_DB=${DATA_DIR}/kamerton.db
KAMERTON_TZ_OFFSET=3
METALOGOS_ENV_ALLOWLIST=${ALLOWLIST}
ENV
    chown "${RUN_AS}:${RUN_AS}" "${DATA_DIR}/env"
    echo "      -> put KAMERTON_TG_TOKEN and KAMERTON_CHAT_ID into ${DATA_DIR}/env"
  fi
else
  echo "      -> exists, left untouched"
fi

step "3/8 mlog ${MLOG_TAG} (SHA256-verified release asset)"
if [ -x /usr/local/bin/mlog ] && echo "${MLOG_SHA}  /usr/local/bin/mlog" | sha256sum -c --status 2>/dev/null; then
  echo "      -> already pinned, left untouched"
else
  tmp="$(mktemp -d)"
  base="https://github.com/ShkodnikAI/Metalogos-/releases/download/${MLOG_TAG}"
  run curl -fsSL -o "${tmp}/mlog" "${base}/mlog-linux-x86_64"
  if [ "${DRY_RUN}" != "1" ]; then
    echo "${MLOG_SHA}  ${tmp}/mlog" | sha256sum -c - >/dev/null
    chmod +x "${tmp}/mlog"
    install -m 0755 "${tmp}/mlog" /usr/local/bin/mlog
  fi
  run rm -rf "${tmp}"
  run mlog --version
fi

step "4/8 app.mlog from the checkout (smoke placeholder if absent)"
if [ -f "${REPO_DIR}/app.mlog" ]; then
  run install -m 0644 "${REPO_DIR}/app.mlog" "${DATA_DIR}/app.mlog"
  run chown "${RUN_AS}:${RUN_AS}" "${DATA_DIR}/app.mlog"
elif [ ! -f "${DATA_DIR}/app.mlog" ]; then
  run install -m 0644 "${REPO_DIR}/deploy/smoke.mlog" "${DATA_DIR}/app.mlog"
  run chown "${RUN_AS}:${RUN_AS}" "${DATA_DIR}/app.mlog"
  echo "      -> installed deploy/smoke.mlog as a placeholder until Н1-06 ships app.mlog"
else
  echo "      -> keeping existing ${DATA_DIR}/app.mlog"
fi

step "5/8 backup script"
run install -m 0755 "${REPO_DIR}/deploy/backup.sh" "${DATA_DIR}/backup.sh"

step "6/8 sqlite3 (restore checks and third-party read-back)"
if command -v sqlite3 >/dev/null 2>&1; then
  echo "      -> present: $(command -v sqlite3)"
else
  run apt-get install -y sqlite3
fi

step "7/8 systemd units + wsl systemd bootstrap"
if [ "${DRY_RUN}" = "1" ]; then
  echo "    + install kamerton.service (User=${RUN_AS}), kamerton-backup.service, kamerton-backup.timer"
else
  sed "s/^\[Service\]$/[Service]\nUser=${RUN_AS}/" "${REPO_DIR}/deploy/kamerton.service" \
    > /etc/systemd/system/kamerton.service
  install -m 0644 "${REPO_DIR}/deploy/kamerton-backup.service" /etc/systemd/system/kamerton-backup.service
  install -m 0644 "${REPO_DIR}/deploy/kamerton-backup.timer" /etc/systemd/system/kamerton-backup.timer
  if ! grep -q '^systemd=true' /etc/wsl.conf 2>/dev/null; then
    printf '[boot]\nsystemd=true\n' >> /etc/wsl.conf
    echo "      -> systemd enabled in /etc/wsl.conf (applies after 'wsl --shutdown' from Windows)"
  fi
  systemctl daemon-reload
  systemctl enable --now systemd-timesyncd.service 2>/dev/null \
    || echo "      -> systemd-timesyncd unavailable; guard the WSL2 clock drift manually"
fi

step "8/8 enable and start"
run systemctl enable kamerton.service kamerton-backup.timer
run systemctl restart kamerton.service
run systemctl start kamerton-backup.timer
if [ "${DRY_RUN}" != "1" ]; then
  systemctl --no-pager --lines=5 status kamerton.service || true
  systemctl list-timers kamerton-backup.timer --no-pager || true
  if ! grep -q '^KAMERTON_TG_TOKEN=.\+' "${DATA_DIR}/env" 2>/dev/null; then
    echo "install: KAMERTON_TG_TOKEN is empty — the Telegram cycle will fail loudly until it is filled in" >&2
  fi
fi

echo "Done. Acceptance (self-host/README.md):"
echo "  1. force-reboot Windows -> systemctl is-active kamerton.service must be 'active';"
echo "  2. the serve banner must print 'route exec: denied' and the env gate state"
echo "     matching deploy/env.allowlist;"
echo "  3. /data/kamerton/backup.sh must restore and read back the database."
