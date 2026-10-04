#!/usr/bin/env bash
# Weekly backup of the Kamerton SQLite database to an external drive,
# with a restore check (naryad Н1-02): the backup is restored into a
# clean directory and read back by a plain sqlite3 client — the same
# check the acceptance criterion asks for. Run from cron/systemd timer:
#   0 3 * * 0  /data/kamerton/backup.sh
set -euo pipefail

DB_PATH=${DB_PATH:-/data/kamerton/kamerton.db}
BACKUP_DIR=${BACKUP_DIR:-/mnt/external/kamerton-backup}
RESTORE_CHECK_DIR=${RESTORE_CHECK_DIR:-/tmp/kamerton-restore-check}
STAMP=$(date -u +%Y%m%dT%H%M%SZ)

if [ ! -f "${DB_PATH}" ]; then
  echo "backup: no database at ${DB_PATH}" >&2
  exit 1
fi
if [ ! -d "${BACKUP_DIR}" ]; then
  echo "backup: external drive not mounted at ${BACKUP_DIR}" >&2
  exit 1
fi

# 1. Consistent copy via the sqlite3 backup API (safe while serving).
sqlite3 "${DB_PATH}" ".backup '${BACKUP_DIR}/kamerton-${STAMP}.db'"

# 2. Restore check into a clean directory, then read it back.
rm -rf "${RESTORE_CHECK_DIR}"
mkdir -p "${RESTORE_CHECK_DIR}"
cp "${BACKUP_DIR}/kamerton-${STAMP}.db" "${RESTORE_CHECK_DIR}/restored.db"
INTEGRITY=$(sqlite3 "${RESTORE_CHECK_DIR}/restored.db" "PRAGMA integrity_check;")
if [ "${INTEGRITY}" != "ok" ]; then
  echo "backup: integrity check failed: ${INTEGRITY}" >&2
  exit 1
fi
TABLES=$(sqlite3 "${RESTORE_CHECK_DIR}/restored.db" \
  "SELECT count(*) FROM sqlite_master WHERE type='table'
   AND name IN ('note','arm','trial','response','context','candidate');")
if [ "${TABLES}" != "6" ]; then
  echo "backup: restored database has ${TABLES}/6 tables" >&2
  exit 1
fi
rm -rf "${RESTORE_CHECK_DIR}"

# 3. Retention: keep the last 8 weekly copies.
ls -1t "${BACKUP_DIR}"/kamerton-*.db 2>/dev/null | tail -n +9 | xargs -r rm -f

echo "backup: ok -> ${BACKUP_DIR}/kamerton-${STAMP}.db (integrity ok, 6 tables readable)"
