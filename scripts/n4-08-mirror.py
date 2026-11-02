#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н4-08: синхронизация telegram-core блока из канона
src/telegram/telegram.mlog в app.mlog (байт-в-байт, вместе с маркерами
APP-MIRROR-START/END: telegram-core). Правки тиков/момента в каноне —
запуск этого скрипта; повторный запуск безопасен. Проверка —
scripts/verify-mirrors.py (префикс app<->tg)."""
import io, re, sys

REPO = "/home/z/my-project/kamerton"

def read(p):
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        return f.read()

def write(p, text):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(text)

def extract(text):
    m = re.search(r"// ── APP-MIRROR-START: telegram-core.*?// ── APP-MIRROR-END: telegram-core[^\n]*\n", text, re.S)
    if not m:
        sys.exit("FAIL: блок telegram-core не найден")
    return m.group(0)

canon = extract(read(REPO + "/src/telegram/telegram.mlog"))
app_path = REPO + "/app.mlog"
t = read(app_path)
old = extract(t)
if old == canon:
    print("SKIP app.mlog: telegram-core уже байт-равен канону")
else:
    t = t.replace(old, canon, 1)
    write(app_path, t)
    print("OK app.mlog: telegram-core заменён (%d -> %d байт)" % (len(old), len(canon)))

if extract(read(app_path)) != canon:
    sys.exit("FAIL: app.mlog: блок после замены не байт-равен канону")
print("ALL GREEN: telegram-core байт-равен в двух файлах")
