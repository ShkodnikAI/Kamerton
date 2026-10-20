#!/usr/bin/env bash
# Kamerton M1 wave-2 deploy driver (Н2-08 + Н2-09 + mlog v0.33.1 pin, PR #68).
#
# One-shot: stop-crane -> backup -> pull -> install.sh -> env -> restart ->
# live checks -> backup. Made for the owner running from PowerShell:
#
#   wsl -e bash -lc "git -C ~/src/kamerton pull --ff-only origin main"
#   wsl -e bash -lc "~/src/kamerton/deploy/deploy-m1-volna2.sh"
#
# Asks for the sudo password once (sudo -v), then reuses the cache.
# Checks are the same as deploy/checklist-m1-volna2.md steps 1-5 + 12 with
# exact predicates instead of the unreliable `grep -c KAMERTON` line counter
# (the allowlist line itself contains KAMERTON_* names, so a bare line count
# can be 5 or 6 depending on how the env file was born).
set -euo pipefail

EXPECTED_HEAD="60c2151bee04fbed9ee50c3d03bd72358720c69a"   # PR #68 squash on main
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR=/data/kamerton
ENV_FILE="$DATA_DIR/env"
DB="$DATA_DIR/kamerton.db"
STAMP="$(date +%Y%m%d-%H%M%S)"
REPORT="$DATA_DIR/deploy-report-$STAMP.txt"

step() { echo; echo "=== [deploy] $* ==="; }
die()  { echo "!!! [deploy] FAIL: $*" >&2; exit 1; }

# --- 0/9 preconditions -----------------------------------------------------
step "0/9 предусловия"
[ "$(id -u)" != "0" ] || die "запускай как обычный пользователь WSL (не через -u root): install.sh требует SUDO_USER"
[ -d "$REPO_DIR/.git" ] || die "$REPO_DIR не похож на чекаут Камертона (нет .git)"
systemctl is-active --quiet kamerton.service || die "kamerton.service не active — сервис болен, деплой не начинаем"
curl -fsS http://127.0.0.1:8461/health >/dev/null || die "health не 200 — деплой не начинаем"
sudo -v || die "sudo-пароль не принят"
echo "ok: сервис active, health 200, sudo прогрет (дальше пароль не спросит)"

# --- 1/9 stop-crane: rollback point ----------------------------------------
step "1/9 стоп-кран: точка отката"
PREV_HEAD="$(git -C "$REPO_DIR" rev-parse HEAD)"
PREV_APP_SHA="$(sha256sum "$DATA_DIR/app.mlog" | cut -d' ' -f1)"
{
  echo "rollback-point $(date -Is)"
  echo "HEAD: $PREV_HEAD"
  echo "app.mlog sha256: $PREV_APP_SHA"
} > "$DATA_DIR/rollback-point.txt"
echo "ok: предыдущий HEAD $PREV_HEAD и sha256 app.mlog записаны в $DATA_DIR/rollback-point.txt"

# --- 2/9 backup before ------------------------------------------------------
step "2/9 бэкап до"
sudo "$DATA_DIR/backup.sh"

# --- 3/9 code ---------------------------------------------------------------
step "3/9 код: pull --ff-only"
git -C "$REPO_DIR" fetch origin
git -C "$REPO_DIR" pull --ff-only origin main
HEAD_NOW="$(git -C "$REPO_DIR" rev-parse HEAD)"
if [ "$HEAD_NOW" != "$EXPECTED_HEAD" ]; then
  echo "ВНИМАНИЕ: HEAD $HEAD_NOW != ожидаемого $EXPECTED_HEAD"
  echo "  (в main мог появиться коммит новее этого скрипта)"
  read -r -p "продолжить деплой на $HEAD_NOW? [y/N] " ans
  [ "$ans" = "y" ] || die "остановлено владельцем"
fi
echo "ok: HEAD $HEAD_NOW"

# --- 4/9 install.sh ---------------------------------------------------------
step "4/9 install.sh (mlog v0.33.1 + свежий app.mlog + юниты)"
sudo bash "$REPO_DIR/deploy/install.sh"
mlog --version 2>/dev/null | grep -q "0.33.1" || die "mlog --version не показал 0.33.1 после install.sh"
echo "ok: mlog на 0.33.1"

# --- 5/9 env ---------------------------------------------------------------
step "5/9 env: KAMERTON_MINIAPP_TOKEN + allowlist"
TOKEN_OK=1; LIST_OK=1
grep -q '^KAMERTON_MINIAPP_TOKEN=.\+' "$ENV_FILE" || TOKEN_OK=0
grep '^METALOGOS_ENV_ALLOWLIST=' "$ENV_FILE" | grep -q 'KAMERTON_MINIAPP_TOKEN' || LIST_OK=0
if [ "$TOKEN_OK" = 1 ] && [ "$LIST_OK" = 1 ]; then
  echo "ok: env уже готов, не трогаю (счётчик строк с KAMERTON: $(grep -c KAMERTON "$ENV_FILE"))"
else
  if [ "$TOKEN_OK" = 0 ]; then
    TOKEN="$(openssl rand -hex 32)"
    echo "KAMERTON_MINIAPP_TOKEN=$TOKEN" >> "$ENV_FILE"
    echo "    >>> СОХРАНИ для закладки телефона (Mini App URL имеет вид ?t=<токен>):"
    echo "    >>> KAMERTON_MINIAPP_TOKEN=$TOKEN"
  fi
  if [ "$LIST_OK" = 0 ]; then
    grep -q '^METALOGOS_ENV_ALLOWLIST=' "$ENV_FILE" || {
      ALLOWLIST="$(grep -vE '^\s*(#|$)' "$REPO_DIR/deploy/env.allowlist" | paste -sd, -)"
      echo "METALOGOS_ENV_ALLOWLIST=$ALLOWLIST" >> "$ENV_FILE"
    }
    sed -i 's/^METALOGOS_ENV_ALLOWLIST=.*/&,KAMERTON_MINIAPP_TOKEN/' "$ENV_FILE"
  fi
  echo "ok: env дополнен (строк с KAMERTON теперь: $(grep -c KAMERTON "$ENV_FILE"))"
fi

# --- 6/9 restart + liveness --------------------------------------------------
step "6/9 рестарт и живость"
sudo systemctl restart kamerton.service
systemctl is-active kamerton.service | grep -q '^active$' || die "сервис не active после рестарта"
curl -fsS http://127.0.0.1:8461/health >/dev/null || die "health не 200 после рестарта"
curl -fsS http://127.0.0.1:8461/jobs | grep -q 'cron' || die "/jobs не вернул cron-выражения"
echo "ok: active, health 200, cron-джобы на месте"
LOG="$(sudo journalctl -u kamerton.service -n 40 --no-pager)"
if echo "$LOG" | grep -q 'ENV_NOT_PERMITTED'; then
  echo "$LOG"
  die "в журнале ENV_NOT_PERMITTED — allowlist неполон; поправь $ENV_FILE и запусти скрипт заново"
fi
if ! echo "$LOG" | grep -q 'route exec: denied'; then
  echo "ВНИМАНИЕ: в баннере не видно 'route exec: denied' — проверь журнал ниже"
fi
echo "$LOG"

# --- 7/9 live circuit --------------------------------------------------------
step "7/9 живой контур: /fire x2 + sqlite"
curl -fsS http://127.0.0.1:8461/fire >/dev/null
curl -fsS http://127.0.0.1:8461/fire >/dev/null
echo "ok: /fire x2 приняты (200 fired)"
PT="$(sqlite3 "$DB" 'SELECT count(*) FROM practice_text;')"
[ "$PT" = "5" ] || die "practice_text=$PT, ожидалось 5"
TRIALS="$(sqlite3 "$DB" 'SELECT trial_date, count(*) AS n FROM trial GROUP BY trial_date ORDER BY trial_date DESC LIMIT 2;')"
echo "trial по датам (верхняя = сегодня):"
echo "$TRIALS"
DUPES="$(sqlite3 "$DB" 'SELECT count(*) FROM (SELECT trial_date FROM trial GROUP BY trial_date HAVING count(*) > 1);')"
[ "$DUPES" = "0" ] || die "есть даты с n>1 — страж Н2-08 нарушен"
echo "ok: practice_text=5, trial n=1 за каждую дату (Н2-08 держит)"

# --- 8/9 backup after ---------------------------------------------------------
step "8/9 бэкап после"
sudo "$DATA_DIR/backup.sh"

# --- 9/9 report ---------------------------------------------------------------
step "9/9 сводка для чата"
{
  echo "Kamerton deploy report $STAMP"
  echo "HEAD: $(git -C "$REPO_DIR" rev-parse HEAD)"
  echo "expected: $EXPECTED_HEAD"
  echo "rollback-point:"
  cat "$DATA_DIR/rollback-point.txt"
  echo "--- trial ---"
  echo "$TRIALS"
  echo "--- practice_text rows: $PT ---"
  echo "--- journalctl -n 40 ---"
  echo "$LOG"
} > "$REPORT"
echo
echo "ГОТОВО. Деплой прошёл все проверки."
echo "Пришли в чат содержимое файла: $REPORT"
echo "(из PowerShell: wsl -e bash -lc \"cat $REPORT\")"
