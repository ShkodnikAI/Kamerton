#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н4-02: синхронизация зеркал из канонов (байт-в-байт, вместе с
маркерами APP-MIRROR-START/END).

  1. arms-seed: канон src/arms/arms.mlog -> app.mlog и
     src/telegram/telegram.mlog (сид MIND-MASTERY каталога).
  2. db-cycle: канон src/telegram/telegram.mlog -> app.mlog
     (ensure_schema/ensure_indexes — три аддитивные таблицы Н4-02).

Правки канонов — запуск этого скрипта; повторный запуск безопасен.
Проверка — scripts/verify-mirrors.py (ALL GREEN обязателен)."""
import io, re, sys

REPO = "/home/z/my-project/kamerton"

def read(p):
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        return f.read()

def write(p, text):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(text)

def block(text, name, end_only=False):
    if end_only:
        m = re.search(r"// ── APP-MIRROR-START: " + re.escape(name) +
                      r".*?// ── APP-MIRROR-END: " + re.escape(name) + r"[^\n]*\n", text, re.S)
    else:
        m = re.search(r"// ── APP-MIRROR-START: " + re.escape(name) +
                      r".*?// ── APP-MIRROR-END: " + re.escape(name) + r"[^\n]*\n", text, re.S)
    if not m:
        sys.exit("FAIL: блок '%s' не найден" % name)
    return m.group(0)

def sync(name, canon_path, targets, end_only=False):
    canon = block(read(canon_path), name, end_only)
    for t in targets:
        t_text = read(t)
        old = block(t_text, name, end_only)
        if old == canon:
            print("SKIP %s: %s уже байт-равен канону" % (name, t))
        else:
            write(t, t_text.replace(old, canon, 1))
            print("OK %s: %s заменён (%d -> %d байт)" % (name, t, len(old), len(canon)))
        if block(read(t), name, end_only) != canon:
            sys.exit("FAIL: %s: блок после замены не байт-равен канону" % t)

ARMS = REPO + "/src/arms/arms.mlog"
APP = REPO + "/app.mlog"
TG = REPO + "/src/telegram/telegram.mlog"

# 1. arms-seed: канон arms.mlog -> app.mlog, telegram.mlog
sync("arms-seed", ARMS, [APP, TG])
# 2. db-cycle: канон telegram.mlog -> app.mlog (db { url } дельта — вне блока)
sync("db-cycle", TG, [APP])

print("ALL GREEN: зеркала Н4-02 синхронизированы")
