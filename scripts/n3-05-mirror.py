#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н3-05: синхронизация arms-seed блока из канона src/arms/arms.mlog
в зеркала app.mlog и src/telegram/telegram.mlog (байт-в-байт, вместе с
маркерами APP-MIRROR-START/END). Запуск после каждой правки блока в
каноне; повторный запуск безопасен. Проверка — scripts/verify-mirrors.py
(KAMERTON_REPO_DIR указывать при необходимости)."""
import io, re, sys

REPO = "/home/z/my-project/kamerton"

def read(p):
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        return f.read()

def write(p, text):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(text)

def extract(text, name):
    m = re.search(
        r"// ── APP-MIRROR-START: " + re.escape(name) +
        r"[^\n]*\n(.*?)// ── APP-MIRROR-END: " + re.escape(name) +
        r"[^\n]*\n", text, re.S)
    if not m:
        sys.exit("FAIL: блок '%s' не найден" % name)
    return m.group(0)  # вместе с маркерами

canon = read(REPO + "/src/arms/arms.mlog")
block = extract(canon, "arms-seed")
print("канон arms-seed: %d байт" % len(block))

for name in ("app.mlog", "src/telegram/telegram.mlog"):
    p = REPO + "/" + name
    t = read(p)
    old = extract(t, "arms-seed")
    if old == block:
        print("SKIP %s: блок уже байт-равен канону" % name)
        continue
    t = t.replace(old, block, 1)
    write(p, t)
    print("OK %s: arms-seed заменён (%d -> %d байт)" % (name, len(old), len(block)))

# самопроверка: во всех трёх файлах блок байт-равен канону
for name in ("app.mlog", "src/telegram/telegram.mlog"):
    t = read(REPO + "/" + name)
    if extract(t, "arms-seed") != block:
        sys.exit("FAIL: %s: блок после замены не байт-равен канону" % name)
print("ALL GREEN: arms-seed байт-равен в трёх файлах")

# Н3-05: DDL arm_journal в ensure_schema обоих файлов (обязательная
# пара db-cycle зеркал; якорь — строка practice_protocol) и в миграции
# 0001_init.sql (аддитивные правила Н1-03). Идемпотентно по якорям.
ddl = "  let _ = db_execute(\"CREATE TABLE IF NOT EXISTS arm_journal (id INTEGER PRIMARY KEY AUTOINCREMENT, actor TEXT DEFAULT '', arm_id INTEGER REFERENCES arm(id), action TEXT DEFAULT '', detail TEXT DEFAULT '', created_at TEXT DEFAULT (datetime('now')))\", [])\n"
for name in ("app.mlog", "src/telegram/telegram.mlog"):
    p = REPO + "/" + name
    t = read(p)
    if "CREATE TABLE IF NOT EXISTS arm_journal" in t:
        print("SKIP %s: DDL arm_journal уже есть" % name)
        continue
    mps = re.search(r"^  let _ = db_execute\(\"CREATE TABLE IF NOT EXISTS practice_protocol[^\n]*\n", t, re.M)
    if not mps:
        sys.exit("FAIL: DDL practice_protocol не найден в %s" % name)
    t = t[:mps.end()] + ddl + t[mps.end():]
    write(p, t)
    print("OK %s: DDL arm_journal добавлен в ensure_schema" % name)

mp = REPO + "/migrations/0001_init.sql"
mig = read(mp)
if "CREATE TABLE IF NOT EXISTS arm_journal" not in mig:
    anchor = re.search(r"CREATE TABLE IF NOT EXISTS practice_protocol \(\n.*?\);\n", mig, re.S)
    if not anchor:
        sys.exit("FAIL: practice_protocol в миграции не найден")
    ddl_sql = """CREATE TABLE IF NOT EXISTS arm_journal (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    actor TEXT DEFAULT '',
    arm_id INTEGER REFERENCES arm(id),
    action TEXT DEFAULT '',
    detail TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);
"""
    mig = mig[:anchor.end()] + "\n" + ddl_sql + mig[anchor.end():]
    write(mp, mig)
    print("OK migrations/0001_init.sql: arm_journal добавлена")
else:
    print("SKIP migrations: arm_journal уже есть")

# pins verify-mirrors.py: arm_journal в списке ddl-pin
vp = REPO + "/scripts/verify-mirrors.py"
vt = read(vp)
if '"arm_journal"' not in vt:
    old_pin = '    ("practice_protocol", "CREATE TABLE IF NOT EXISTS practice_protocol (id INTEGER"),\n'
    new_pin = old_pin + '    ("arm_journal",     "CREATE TABLE IF NOT EXISTS arm_journal (id INTEGER"),\n'
    if old_pin not in vt:
        sys.exit("FAIL: якорь pins в verify-mirrors.py не найден")
    vt = vt.replace(old_pin, new_pin, 1)
    write(vp, vt)
    print("OK scripts/verify-mirrors.py: arm_journal добавлена в ddl-pin")
else:
    print("SKIP verify-mirrors.py: arm_journal уже в pins")
