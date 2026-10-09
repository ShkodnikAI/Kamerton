#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н2-07: зеркалирование H2-07-SPECTRAL блока из канона
src/telegram/telegram.mlog в app.mlog (telegram-core). Идемпотентен:
заменяет существующую копию."""
import io, re, sys

REPO = "/home/z/my-project/kamerton"

def read(p):
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        return f.read()

def write(p, text):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(text)

canon = read(REPO + "/src/telegram/telegram.mlog")
m = re.search(r"// ── H2-07-SPECTRAL-START.*?// ── H2-07-SPECTRAL-END[^\n]*\n", canon, re.S)
if not m:
    sys.exit("FAIL: H2-07-SPECTRAL блок не найден в каноне")
block = m.group(0)

p = REPO + "/app.mlog"
t = read(p)
# снести старую копию
t = re.sub(r"// ── H2-07-SPECTRAL-START.*?// ── H2-07-SPECTRAL-END[^\n]*\n", "", t, count=1, flags=re.S)
# вставка на ТОЙ ЖЕ относительной позиции, что в каноне: перед
# pattern process_updates (внутри telegram-core), иначе префикс-проверка
# app<->tg разъезжается
anchor = "pattern process_updates(updates: List<public, trusted>, chat_id: String<public, trusted>, local_date: String<public, trusted>, offset: Float) -> Unit {"
idx = t.find(anchor)
if idx < 0:
    sys.exit("FAIL: якорь process_updates не найден в app.mlog")
t = t[:idx] + block + "\n" + t[idx:]
write(p, t)

# самопроверка: копия байт-равна канону
app2 = read(p)
m2 = re.search(r"// ── H2-07-SPECTRAL-START.*?// ── H2-07-SPECTRAL-END[^\n]*\n", app2, re.S)
if m2 and m2.group(0) == block:
    print("OK app.mlog: H2-07-SPECTRAL байт-равен канону (%d байт)" % len(block))
else:
    sys.exit("FAIL: копия различается")
print("MIRROR-H2-07: DONE")
