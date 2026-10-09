#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н2-06: вставка sweep-блока в зеркала store-core (app.mlog + telegram.mlog)
и DDL sweep_result в ensure_schema обоих файлов. Копия берётся ПОБАЙТНО из
канона src/store/store.mlog (от заголовка Sweep protocol до конца
sweep_record, перед "// ── Tests"). Запуск один раз; повторный запуск
безопасен (идемпотентен по якорям)."""
import io, re, sys

REPO = "/home/z/my-project/kamerton"

def read(p):
    with io.open(p, "r", encoding="utf-8", newline="") as f:
        return f.read()

def write(p, text):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(text)

canon = read(REPO + "/src/store/store.mlog")
m = re.search(r"// ── Sweep protocol \(Н2-06\).*?(?=// ── Tests)", canon, re.S)
if not m:
    sys.exit("FAIL: sweep-блок в каноне не найден")
sweep_block = m.group(0).rstrip("\n") + "\n"

# 1) store-core ×2: замена существующего sweep-блока (или вставка) перед END-маркером
for name in ("app.mlog", "src/telegram/telegram.mlog"):
    p = REPO + "/" + name
    t = read(p)
    anchor = "// ── APP-MIRROR-END: store-core"
    idx = t.find(anchor)
    if idx < 0:
        sys.exit("FAIL: якорь store-core не найден в %s" % name)
    # снести старую копию блока, если она есть
    pat = re.compile(r"// ── Sweep protocol \(Н2-06\).*?(?=\n// ── APP-MIRROR-END: store-core)", re.S)
    t = pat.sub("", t, count=1)
    idx = t.find(anchor)
    t = t[:idx] + sweep_block + "\n" + t[idx:]
    write(p, t)
    print("OK %s: sweep-блок заменён в store-core (%d байт)" % (name, len(sweep_block)))

# 2) ensure_schema ×2: DDL sweep_result после строки practice_session
ddl = "  let _ = db_execute(\"CREATE TABLE IF NOT EXISTS sweep_result (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id INTEGER REFERENCES practice_session(id), result_date TEXT DEFAULT '', resonance_bpm REAL DEFAULT '0.0', peak_freq_bpm REAL DEFAULT '0.0', power_fraction REAL DEFAULT '0.0', p_value REAL DEFAULT '1.0', steps_valid INTEGER DEFAULT '0', steps_total INTEGER DEFAULT '0', prev_resonance_bpm REAL DEFAULT '0.0', freq_delta_bpm REAL DEFAULT '0.0', share_ratio REAL DEFAULT '0.0', verdict TEXT DEFAULT '', created_at TEXT DEFAULT (datetime('now')))\", [])\n"
for name in ("app.mlog", "src/telegram/telegram.mlog"):
    p = REPO + "/" + name
    t = read(p)
    if "CREATE TABLE IF NOT EXISTS sweep_result" in t:
        print("SKIP %s: DDL уже есть" % name)
        continue
    mps = re.search(r"^  let _ = db_execute\(\"CREATE TABLE IF NOT EXISTS practice_session.*?\n", t, re.M)
    if not mps:
        sys.exit("FAIL: DDL practice_session не найден в %s" % name)
    t = t[:mps.end()] + ddl + t[mps.end():]
    write(p, t)
    print("OK %s: DDL sweep_result добавлен в ensure_schema" % name)

# 3) самопроверка: вставленные блоки байт-равны и канону, и друг другу
app = read(REPO + "/app.mlog")
tg = read(REPO + "/src/telegram/telegram.mlog")
def extract(text):
    return re.search(r"// ── Sweep protocol \(Н2-06\).*?(?=\n// ── APP-MIRROR-END: store-core)", text, re.S).group(0).rstrip("\n") + "\n"
b_app, b_tg = extract(app), extract(tg)
if b_app == b_tg == sweep_block:
    print("OK: sweep-блок байт-равен канону и обоим зеркалам")
else:
    sys.exit("FAIL: вставленные блоки различаются")
print("MIRROR-H2-06: DONE")
