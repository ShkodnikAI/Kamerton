#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н4-07: синхронизация зеркал из канонов (байт-в-байт, вместе с
маркерами APP-MIRROR-START/END).

  1. arms-seed:    канон src/arms/arms.mlog -> app.mlog и
                   src/telegram/telegram.mlog (practice_admission в
                   seed_mm_catalog; mm_admission_ok; гейт activate_arm).
  2. bandit-core:  ротационный фрагмент канона src/bandit/bandit.mlog
                   (Н3-06 заголовок -> Test fixtures) -> app.mlog и
                   src/telegram/telegram.mlog (admission-предикат в
                   rotation_candidate).
  3. miniapp-core: канон src/miniapp/miniapp.mlog -> app.mlog и
                   src/telegram/telegram.mlog (consent_disclaimer_record/
                   present, emergency_stop).
  4. db-cycle:     канон src/telegram/telegram.mlog -> app.mlog
                   (ensure_schema/ensure_indexes — practice_admission;
                   ветка «стоп» в process_updates).
  5. telegram-core: канон src/telegram/telegram.mlog -> app.mlog
                   (parse_chat_text «стоп»; morning_proposal_row contra;
                   format_proposal 4-арг).

Литерал страницы Mini App обновляет scripts/gen-page-literal.py (три
копии miniapp_html). Остальные блоки Н4-07 не трогает.

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

# 1. arms-seed: канон src/arms/arms.mlog -> app.mlog и telegram.mlog
sync("arms-seed", REPO + "/src/arms/arms.mlog", [APP, TG])

# 2. bandit-core: ротационный фрагмент канона src/bandit/bandit.mlog
mb = re.search(r"(// ── Н3-06: slot rotation.*?)\n// ── Test fixtures", read(REPO + "/src/bandit/bandit.mlog"), re.S)
if not mb:
    sys.exit("FAIL: ротационный фрагмент Н3-06/Н4-07 в каноне bandit не найден")
fragment = mb.group(1).rstrip("\n") + "\n"
for name in ("app.mlog", "src/telegram/telegram.mlog"):
    p = REPO + "/" + name
    t = read(p)
    anchor = "// ── APP-MIRROR-END: bandit-core"
    pat = re.compile(r"// ── Н3-06: slot rotation.*?(?=\n// ── APP-MIRROR-END: bandit-core)", re.S)
    t2 = pat.sub("", t, count=1)
    idx = t2.find(anchor)
    if idx < 0:
        sys.exit("FAIL: якорь bandit-core не найден в %s" % name)
    t2 = t2[:idx] + fragment + "\n" + t2[idx:]
    if t2 == t:
        print("SKIP %s: фрагмент уже синхронен" % name)
        continue
    write(p, t2)
    print("OK %s: фрагмент Н3-06/Н4-07 синхронизирован (%d байт)" % (name, len(fragment)))

# 3. miniapp-core: канон src/miniapp/miniapp.mlog -> app.mlog и telegram.mlog
sync("miniapp-core", REPO + "/src/miniapp/miniapp.mlog", [APP, TG])

# 4. db-cycle: канон telegram.mlog -> app.mlog (db { url } дельта — вне блока)
sync("db-cycle", TG, [APP])

# 5. telegram-core: канон telegram.mlog -> app.mlog
sync("telegram-core", TG, [APP])

print("ALL GREEN: зеркала Н4-07 синхронизированы")
