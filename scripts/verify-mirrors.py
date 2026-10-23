#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Верификатор APP-MIRROR дисциплины Камертона (Н2-01).

Проверяет строго (exit 1 при любом промахе):
  1. arms-seed: блок между маркерами APP-MIRROR-START/END: arms-seed
     байт-в-байт равен во всех трёх файлах:
       src/arms/arms.mlog (канон), app.mlog, src/telegram/telegram.mlog.
  2. app↔telegram: общий префикс app.mlog (от первой строки до строки
     'APP-MIRROR-END: telegram-core' включительно) байт-равен
     соответствующему префиксу src/telegram/telegram.mlog, ЗА ИСКЛЮЧЕНИЕМ
     ровно одной заявленной дельты — строки db { url: ... } (две лейны,
     §2-затык Н1-06); дельта обязана быть на одной и той же строке в обоих
     файлах и начинаться с 'db { url:'.

Запуск: python3 scripts/verify-mirrors.py
"""
import io, os, re, sys

REPO = os.environ.get("KAMERTON_REPO_DIR", "/home/z/my-project/kamerton")
FILES = {
    "arms": REPO + "/src/arms/arms.mlog",
    "app":  REPO + "/app.mlog",
    "tg":   REPO + "/src/telegram/telegram.mlog",
}

def read(p):
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        return f.read()

def block(text, name):
    m = re.search(
        r"// ── APP-MIRROR-START: " + re.escape(name) +
        r"[^\n]*\n(.*?)// ── APP-MIRROR-END: " + re.escape(name) +
        r"[^\n]*\n", text, re.S)
    if not m:
        sys.exit("FAIL: блок '%s' не найден" % name)
    return m.group(0)  # блок ВМЕСТЕ с маркерами — сравниваем целиком

fail = 0

# ── 1. arms-seed и miniapp-core байт-в-байт в трёх экземплярах ──────────
MIRRORS = {
    "arms-seed": [REPO + "/src/arms/arms.mlog", FILES["app"], FILES["tg"]],
    "miniapp-core": [REPO + "/src/miniapp/miniapp.mlog", FILES["app"], FILES["tg"]],
    # Н2-13: слепой контекст и клапан честности в рабочем приложении.
    "context-core": [REPO + "/src/context/context.mlog", FILES["app"], FILES["tg"]],
    "honesty-report-core": [REPO + "/src/report/report.mlog", FILES["app"], FILES["tg"]],
}
for name, paths in MIRRORS.items():
    bs = [block(read(p), name) for p in paths]
    if bs[0] == bs[1] == bs[2]:
        print("OK %s: блок байт-равен в трёх файлах (%d байт)" % (name, len(bs[0])))
    else:
        print("FAIL %s: блоки различаются" % name)
        fail = 1

# ── 2. префикс app↔telegram с единственной заявленной дельтой ───────────
app_lines = read(FILES["app"]).split("\n")
tg_lines  = read(FILES["tg"]).split("\n")
end_idx = next((i for i, l in enumerate(app_lines)
                if l.startswith("// ── APP-MIRROR-END: telegram-core")), None)
if end_idx is None:
    sys.exit("FAIL: якорь END: telegram-core не найден в app.mlog")
prefix_len = end_idx + 1
diffs = []
for i in range(prefix_len):
    a = app_lines[i] if i < len(app_lines) else None
    t = tg_lines[i] if i < len(tg_lines) else None
    if a != t:
        diffs.append((i + 1, a, t))
if len(diffs) == 1:
    ln, a, t = diffs[0]
    if (a is not None and t is not None
            and a.startswith("db { url:") and t.startswith("db { url:")):
        print("OK app<->tg: префикс %d строк байт-равен, заявленная дельта одна "
              "(строка %d, db-url двух лейн)" % (prefix_len, ln))
    else:
        print("FAIL app<->tg: дельта на строке %d не является заявленной db-url" % ln)
        print("  app: %r" % a)
        print("  tg : %r" % t)
        fail = 1
elif len(diffs) == 0:
    print("FAIL app<->tg: префиксы равны ЦЕЛИКОМ — пропала заявленная дельта "
          "db-url (так не бывает: у app env(), у tg литерал)")
    fail = 1
else:
    print("FAIL app<->tg: %d расхождений (разрешена ровно одна db-url):" % len(diffs))
    for ln, a, t in diffs[:10]:
        print("  строка %d:\n    app: %r\n    tg : %r" % (ln, a, t))
    fail = 1

# ── 3. DDL practice_session и sweep_result присутствуют и в ensure_schema,
#       и в миграции ────────────────────────────────────────────────────────
app_text = read(FILES["app"])
mig_text = read(REPO + "/migrations/0001_init.sql")
pins = [
    ("practice_session", "CREATE TABLE IF NOT EXISTS practice_session (id INTEGER"),
    ("sweep_result",     "CREATE TABLE IF NOT EXISTS sweep_result (id INTEGER"),
    ("practice_text",    "CREATE TABLE IF NOT EXISTS practice_text (id INTEGER"),
]
for table, pin_a in pins:
    pin_m = "CREATE TABLE IF NOT EXISTS " + table + " ("
    ok_a = pin_a in app_text and "ensure_schema" in app_text
    ok_m = pin_m in mig_text
    if ok_a and ok_m:
        print("OK ddl-pin: %s заявлена в ensure_schema (app) и в миграции" % table)
    else:
        print("FAIL ddl-pin %s: app=%s migration=%s" % (table, ok_a, ok_m))
        fail = 1

if fail:
    sys.exit(1)
print("MIRRORS: ALL GREEN")
