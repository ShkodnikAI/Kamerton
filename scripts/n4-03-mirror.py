#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н4-03: синхронизация зеркал из канонов (байт-в-байт, вместе с
маркерами APP-MIRROR-START/END).

  db-cycle:      канон src/telegram/telegram.mlog -> app.mlog
                 (ensure_schema/ensure_indexes — две аддитивные таблицы
                 Н4-03; паттерны плана и дневника; диспетчер команд
                 process_updates; hoist msg_ts; счётчик plans).
  telegram-core: канон src/telegram/telegram.mlog -> app.mlog
                 (parse_chat_text — план/добавить/сделано/d:;
                 format_plan/candidates/notfound/hint).

Литерал страницы Mini App обновляет scripts/gen-page-literal.py (три
копии miniapp-core). Остальные блоки Н4-03 не трогает.

Правки канона — запуск этого скрипта; повторный запуск безопасен.
Проверка — scripts/verify-mirrors.py (ALL GREEN обязателен)."""
import io, re, sys

REPO = "/home/z/my-project/kamerton"

def read(p):
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        return f.read()

def write(p, text):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(text)

def block(text, name):
    m = re.search(r"// ── APP-MIRROR-START: " + re.escape(name) +
                  r".*?// ── APP-MIRROR-END: " + re.escape(name) + r"[^\n]*\n", text, re.S)
    if not m:
        sys.exit("FAIL: блок '%s' не найден" % name)
    return m.group(0)

def sync(name, canon_path, targets):
    canon = block(read(canon_path), name)
    for t in targets:
        t_text = read(t)
        old = block(t_text, name)
        if old == canon:
            print("SKIP %s: %s уже байт-равен канону" % (name, t))
        else:
            write(t, t_text.replace(old, canon, 1))
            print("OK %s: %s заменён (%d -> %d байт)" % (name, t, len(old), len(canon)))
        if block(read(t), name) != canon:
            sys.exit("FAIL: %s: блок после замены не байт-равен канону" % t)

APP = REPO + "/app.mlog"
TG = REPO + "/src/telegram/telegram.mlog"

# 1. db-cycle: канон telegram.mlog -> app.mlog (db { url } дельта — вне блока)
sync("db-cycle", TG, [APP])
# 2. telegram-core: канон telegram.mlog -> app.mlog
sync("telegram-core", TG, [APP])

print("ALL GREEN: зеркала Н4-03 синхронизированы")
