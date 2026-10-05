#!/usr/bin/env bash
# Kamerton weekly SQLite backup with a built-in restore check (naryad Н1-02).
#
# Per the acceptance criterion, every run restores the fresh backup into a
# clean directory and reads it back with a plain sqlite3 client (invariant
# 6: the database must be readable by a third-party tool, not only by the
# app). Schema-level assertions (the six tables) belong to naryad Н1-03;
# this script asserts integrity and readability only.
#
# Runs from the systemd timer kamerton-backup.timer (Sundays 03:00, WSL2).
# Retention: the last 8 weekly copies are kept on the external drive.
#
# Overrides for testing:
#   DB_PATH, BACKUP_DIR, RESTORE_CHECK_DIR, SQLITE3
set -euo pipefail

DB_PATH=${DB_PATH:-/data/kamerton/kamerton.db}
BACKUP_DIR=${BACKUP_DIR:-/mnt/external/kamerton-backup}
RESTORE_CHECK_DIR=${RESTORE_CHECK_DIR:-/tmp/kamerton-restore-check}
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
SQLITE3=${SQLITE3:-$(command -v sqlite3 || true)}

if [ -z "${SQLITE3}" ] || [ ! -x "${SQLITE3}" ]; then
  echo "backup: sqlite3 not found — install it (deploy/install.sh step 6)" >&2
  exit 1
fi
if [ ! -f "${DB_PATH}" ]; then
  echo "backup: no database at ${DB_PATH}" >&2
  exit 1
fi
if [ ! -d "${BACKUP_DIR}" ]; then
  echo "backup: external drive not mounted at ${BACKUP_DIR}" >&2
  exit 1
fi

# 1. Consistent copy via the sqlite3 backup API (safe while serving).
"${SQLITE3}" "${DB_PATH}" ".backup '${BACKUP_DIR}/kamerton-${STAMP}.db'"

# 2. Restore check into a clean directory, then read it back.
rm -rf "${RESTORE_CHECK_DIR}"
mkdir -p "${RESTORE_CHECK_DIR}"
cp "${BACKUP_DIR}/kamerton-${STAMP}.db" "${RESTORE_CHECK_DIR}/restored.db"
INTEGRITY=$("${SQLITE3}" "${RESTORE_CHECK_DIR}/restored.db" "PRAGMA integrity_check;")
if [ "${INTEGRITY}" != "ok" ]; then
  echo "backup: integrity check failed: ${INTEGRITY}" >&2
  exit 1
fi
READBACK=$("${SQLITE3}" "${RESTORE_CHECK_DIR}/restored.db" \
  "SELECT count(*) FROM sqlite_master;")
case "${READBACK}" in
  ''|*[!0-9]*) echo "backup: restored database is not readable by sqlite3" >&2; exit 1 ;;
esac
rm -rf "${RESTORE_CHECK_DIR}"

# 3. Retention: keep the last 8 weekly copies (newest by mtime).
find "${BACKUP_DIR}" -maxdepth 1 -name 'kamerton-*.db' -printf '%T@\t%p\n' 2>/dev/null \
  | sort -rn | tail -n +9 | cut -f2- | xargs -r rm -f

echo "backup: ok -> ${BACKUP_DIR}/kamerton-${STAMP}.db (integrity ok, sqlite3 read-back ok, ${READBACK} schema objects)"
