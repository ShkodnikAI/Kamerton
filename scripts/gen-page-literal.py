#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н2-03/Н2-06: регенерация однострочного литерала страницы в трёх зеркалах
miniapp_html() — src/miniapp/miniapp.mlog (канон блока), app.mlog,
src/telegram/telegram.mlog. Источник — self-host/miniapp/index.html.
Экранирование — JSON-совместимое (\", \\, \\n); юникод остаётся сырым.
Запуск после КАЖДОЙ правки index.html, затем scripts/verify-mirrors.py."""
import io, json, re, sys

REPO = "/home/z/my-project/kamerton"

def read(p):
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        return f.read()

def write(p, text):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(text)

page = read(REPO + "/self-host/miniapp/index.html")
if "http://" in page or "https://" in page:
    sys.exit("FAIL: внешний origin в странице (инвариант 7)")
escaped = json.dumps(page, ensure_ascii=False)[1:-1]
literal = '  return "' + escaped + '"\n'

FILES = ["src/miniapp/miniapp.mlog", "app.mlog", "src/telegram/telegram.mlog"]
for name in FILES:
    p = REPO + "/" + name
    t = read(p)
    new_t, n = re.subn(r'  return "<!doctype html>\\n<html lang=\\"ru\\">.*"(?=\n)', lambda mm: literal.rstrip("\n"), t, count=1)
    if n != 1:
        sys.exit("FAIL: литерал не найден в %s" % name)
    write(p, new_t)
    print("OK %s: литерал регенерирован (%d байт страницы)" % (name, len(page)))
print("PAGE-LITERAL: DONE")
