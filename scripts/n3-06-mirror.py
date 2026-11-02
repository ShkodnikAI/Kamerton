#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н3-06: синхронизация ротационного блока канона src/bandit/bandit.mlog
в зеркала app.mlog и src/telegram/telegram.mlog (фрагмент Н3-06 от заголовка
«// ── Н3-06: slot rotation» до «// ── Test fixtures» вставляется перед
«// ── APP-MIRROR-END: bandit-core»). Запуск после каждой правки блока в
каноне; повторный запуск безопасен. Проверка — scripts/verify-mirrors.py
(проверка bandit-core добавлена в Н3-06)."""
import io, re, sys

REPO = "/home/z/my-project/kamerton"

def read(p):
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        return f.read()

def write(p, text):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(text)

canon = read(REPO + "/src/bandit/bandit.mlog")
m = re.search(r"(// ── Н3-06: slot rotation.*?)\n// ── Test fixtures", canon, re.S)
if not m:
    sys.exit("FAIL: ротационный фрагмент Н3-06 в каноне bandit.mlog не найден")
fragment = m.group(1).rstrip("\n") + "\n"
print("канон Н3-06 фрагмент: %d байт" % len(fragment))

for name in ("app.mlog", "src/telegram/telegram.mlog"):
    p = REPO + "/" + name
    t = read(p)
    anchor = "// ── APP-MIRROR-END: bandit-core"
    idx = t.find(anchor)
    if idx < 0:
        sys.exit("FAIL: якорь bandit-core не найден в %s" % name)
    # снести старую копию фрагмента, если она есть
    pat = re.compile(r"// ── Н3-06: slot rotation.*?(?=\n// ── APP-MIRROR-END: bandit-core)", re.S)
    t2 = pat.sub("", t, count=1)
    idx = t2.find(anchor)
    t2 = t2[:idx] + fragment + "\n" + t2[idx:]
    if t2 == t:
        print("SKIP %s: фрагмент уже синхронен" % name)
        continue
    write(p, t2)
    print("OK %s: Н3-06 фрагмент синхронизирован (%d байт)" % (name, len(fragment)))

# самопроверка: фрагмент в обоих зеркалах байт-равен канону
for name in ("app.mlog", "src/telegram/telegram.mlog"):
    t = read(REPO + "/" + name)
    mm = re.search(r"(// ── Н3-06: slot rotation.*?)\n// ── APP-MIRROR-END: bandit-core", t, re.S)
    if not mm or mm.group(1).rstrip("\n") + "\n" != fragment:
        sys.exit("FAIL: %s: фрагмент после вставки не байт-равен канону" % name)
print("ALL GREEN: Н3-06 фрагмент байт-равен в трёх файлах")
