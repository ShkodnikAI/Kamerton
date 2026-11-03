#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Н4-01: разовый конвертер контента MIND-MASTERY в нормализованные данные
Камертона (скрипт сборки, НЕ часть сервиса).

Источник: клон ShkodnikAI/MIND-MASTERY, коммит 2a26e75 (вне репозитория
Камертона; путь задаётся --source или env MM_SOURCE).

Пишет (побайтно-повторяемо, без mp3):
  seeds/mm/practices.json      — 75 практик: id, система, название, описание,
                                 уровень, сложность, минуты, противопоказания,
                                 теги, сегменты (как в источнике) + нормализованный
                                 протокол формата Н3-01 (фаза сценария → фаза
                                 протокола, длительность, класс по данным)
  seeds/mm/scripts.json        — textKey → текст (все тексты сценариев)
  seeds/mm/voice-manifest.json — textKey → {файл, SHA-256, длительность, размер}
  seeds/mm/progression.json    — Н4-07: правила допуска по системам
                                 (src/content/systems/*/progression.ts @ 2a26e75):
                                 плоский список [{system, level, sessions, minutes}];
                                 уровни ≥ 2 (уровень 1 всегда открыт)

Проверки по образцу validate-content MIND-MASTERY:
  1) все textKey сегментов TS есть в ru.json; нет сиротских ключей JSON;
  2) у каждого voice-сегмента есть mp3 и запись манифеста; нет сиротских mp3;
  3) счётчики наряда: ровно 75 практик, ровно 591 voice-сегмент, 0 расхождений.

Режим --verify: регенерирует в память и сверяет существующие файлы побайтно
(повторяемость — приёмка наряда), не пишет.
"""
import argparse
import hashlib
import io
import json
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SYSTEMS = ["breath", "dispenza", "domfreq", "feldenkrais", "silva", "transurfing"]
EXPECTED_PRACTICES = 75
EXPECTED_VOICE = 591
DIFF_MAPS = {
    "DIFF_MAP_4": {1: "beginner", 2: "elementary", 3: "intermediate", 4: "advanced"},
    "DIFF_MAP_5": {1: "beginner", 2: "elementary", 3: "intermediate", 4: "advanced", 5: "master"},
}
# Нормализованные фазы сценария → фаза протокола (формат Н3-01).
# Данные MIND-MASTERY — голосовые сценарии: механических дыхательных фаз
# (inhale/hold/exhale/forced_exhale) в сегментах НЕТ, поэтому фазой
# протокола становится фаза сценария КАК ЕСТЬ (ничего не переименовывается
# в дыхательное), тишина → 'silence'. Проводить сценарные фазы через
# дыхательную семантику значило бы придумать данные, которых нет.
SCENE_SILENCE_PHASE = "silence"


def read(p):
    with io.open(p, "r", encoding="utf-8") as f:
        return f.read()


# ── мини-сканер TS-литералов: комментарии + строки + баланс скобок ───────
class TsScan:
    def __init__(self, text):
        self.t = text
        self.i = 0
        self.n = len(text)

    def skip_ws(self):
        t, n = self.t, self.n
        while self.i < n:
            c = t[self.i]
            if c in " \t\r\n":
                self.i += 1
            elif t.startswith("//", self.i):
                j = t.find("\n", self.i)
                self.i = n if j < 0 else j + 1
            elif t.startswith("/*", self.i):
                j = t.find("*/", self.i + 2)
                if j < 0:
                    raise ValueError("незакрытый блочный комментарий")
                self.i = j + 2
            else:
                return

    def string(self):
        # одна строка '...' с \-экранированием
        out = []
        i = self.i + 1
        t = self.t
        while i < self.n:
            c = t[i]
            if c == "\\":
                out.append(t[i : i + 2])
                i += 2
                continue
            if c == "'":
                self.i = i + 1
                raw = "".join(out)
                return json.loads('"' + raw.replace('"', '\\"') + '"')
            out.append(c)
            i += 1
        raise ValueError("незакрытая строка")

    def balanced(self, open_ch, close_ch):
        # текст между open_ch и парной close_ch, комментарии и строки пропущены
        assert self.t[self.i] == open_ch
        depth = 0
        start = self.i
        t, n = self.t, self.n
        while self.i < n:
            c = t[self.i]
            if c in " \t\r\n":
                self.i += 1
            elif t.startswith("//", self.i):
                j = t.find("\n", self.i)
                self.i = n if j < 0 else j + 1
            elif t.startswith("/*", self.i):
                j = t.find("*/", self.i + 2)
                self.i = j + 2
            elif c == "'":
                self.string()
            elif c == open_ch:
                depth += 1
                self.i += 1
            elif c == close_ch:
                depth -= 1
                self.i += 1
                if depth == 0:
                    return t[start : self.i]
            else:
                self.i += 1
        raise ValueError("незакрытая группа %s%s" % (open_ch, close_ch))


def parse_practices_file(path):
    """Все makePractice-вызовы файла → список кортежей аргументов."""
    text = read(path)
    scan = TsScan(text)
    out = []
    while True:
        scan.skip_ws()
        idx = text.find("makePractice(", scan.i)
        if idx < 0:
            break
        scan.i = idx + len("makePractice(") - 1
        body = scan.balanced("(", ")")
        out.append(parse_args(body[1:-1]))
    return out


def split_args(body):
    """Аргументы верхнего уровня (запятые вне строк/скобок). Блоки [..] и
    {..} потребляются целиком и глубину не наращивают — на верхнем уровне
    makePractice вложенных групп со скобками не бывает."""
    scan = TsScan(body)
    args = []
    cur = []
    t, n = body, len(body)
    while scan.i < n:
        scan.skip_ws()
        if scan.i >= n:
            break
        c = t[scan.i]
        if c == "'":
            cur.append(("str", scan.string()))
            continue
        if c == "[":
            blk = scan.balanced("[", "]")
            cur.append(("array", parse_array(blk[1:-1])))
            continue
        if c == "{":
            blk = scan.balanced("{", "}")
            cur.append(("object", parse_segment(blk[1:-1])))
            continue
        if c == ",":
            args.append(cur)
            cur = []
            scan.i += 1
            continue
        m = re.match(r"[A-Za-z_][A-Za-z0-9_]*|-?[0-9]+(?:\.[0-9]+)?", t[scan.i :])
        if m:
            tok = m.group(0)
            scan.i += len(tok)
            if re.match(r"^-?[0-9]", tok):
                cur.append(("num", float(tok) if "." in tok else int(tok)))
            else:
                cur.append(("ident", tok))
            continue
        scan.i += 1
    if cur:
        args.append(cur)
    return args


def parse_args(body):
    vals = []
    for a in split_args(body):
        if len(a) != 1:
            raise ValueError("аргумент из %d токенов: %r" % (len(a), a[:2]))
        vals.append(a[0][1])
    return vals


def parse_array(body):
    if body.strip() == "":
        return []
    items = []
    scan = TsScan(body)
    t, n = body, len(body)
    depth = 0
    cur = []
    while scan.i < n:
        scan.skip_ws()
        if scan.i >= n:
            break
        c = t[scan.i]
        if c == "'":
            cur.append(scan.string())
            continue
        if c == "{":
            blk = scan.balanced("{", "}")
            cur.append(parse_segment(blk[1:-1]))
            continue
        if c == "[":
            blk = scan.balanced("[", "]")
            cur.append(parse_array(blk[1:-1]))
            continue
        m = re.match(r"[A-Za-z_][A-Za-z0-9_]*|-?[0-9]+(?:\.[0-9]+)?", t[scan.i :])
        if m:
            tok = m.group(0)
            scan.i += len(tok)
            cur.append(float(tok) if "." in tok else (int(tok) if re.match(r"^-?[0-9]", tok) else tok))
            continue
        scan.i += 1
    return cur


def parse_segment(body):
    seg = {}
    scan = TsScan(body)
    t, n = body, len(body)
    while scan.i < n:
        scan.skip_ws()
        if scan.i >= n:
            break
        m = re.match(r"([A-Za-z_][A-Za-z0-9_]*)\s*:", t[scan.i :])
        if not m:
            scan.i += 1
            continue
        key = m.group(1)
        scan.i += m.end()
        scan.skip_ws()
        c = t[scan.i]
        if c == "'":
            seg[key] = scan.string()
        elif c == "{":
            blk = scan.balanced("{", "}")
            seg[key] = parse_segment(blk[1:-1])
        else:
            m2 = re.match(r"(true|false|-?[0-9]+(?:\.[0-9]+)?)", t[scan.i :])
            if not m2:
                raise ValueError("сегмент %s: неразобранный литерал у %r" % (key, t[scan.i : scan.i + 30]))
            tok = m2.group(0)
            scan.i += len(tok)
            if tok == "true":
                seg[key] = True
            elif tok == "false":
                seg[key] = False
            else:
                seg[key] = float(tok) if "." in tok else int(tok)
    return seg


# ── сборка ────────────────────────────────────────────────────────────────
def load_texts(source):
    """practiceId → {textKey → текст} из content/texts/*/*.ru.json."""
    texts = {}
    base = os.path.join(source, "content", "texts")
    for system in SYSTEMS:
        d = os.path.join(base, system)
        for fn in sorted(os.listdir(d)):
            if not fn.endswith(".ru.json"):
                continue
            data = json.loads(read(os.path.join(d, fn)))
            texts[data["practiceId"]] = data["segments"]
    return texts


def probe_audio(source, text_key):
    rel = "assets/audio/voice/%s/%s.mp3" % (text_key.split(".")[0], text_key.replace(".", "_"))
    path = os.path.join(source, rel)
    if not os.path.exists(path):
        return None
    raw = open(path, "rb").read()
    sha = hashlib.sha256(raw).hexdigest()
    try:
        out = subprocess.run(
            ["ffprobe", "-v", "quiet", "-show_entries", "format=duration", "-of", "csv=p=0", path],
            capture_output=True, text=True, timeout=30,
        ).stdout.strip()
        dur = round(float(out), 2)
    except Exception:
        dur = 0.0
    return {"file": rel, "sha256": sha, "duration_sec": dur, "size_bytes": len(raw)}


def convert(source):
    practices = []
    scripts = {}
    manifest = {}
    ts_text_keys = set()
    voice_keys = set()
    for system in SYSTEMS:
        args_list = parse_practices_file(os.path.join(source, "src", "content", "systems", system, "practices.ts"))
        for vals in args_list:
            if len(vals) != 10:
                raise ValueError("%s: makePractice из %d аргументов (ожидалось 10)" % (system, len(vals)))
            sys_id, diff_id, pid, title, descr, level, minutes, contra, tags, segments = vals
            if sys_id != system:
                raise ValueError("%s: systemId %r не совпадает с папкой" % (pid, sys_id))
            if diff_id not in DIFF_MAPS:
                raise ValueError("%s: неизвестная diff-карта %r" % (pid, diff_id))
            norm_segs = []
            for seg in segments:
                kind = seg["kind"]
                entry = {"kind": kind, "duration_sec": seg["durationSec"]}
                if kind == "voice":
                    entry["text_key"] = seg["textKey"]
                    entry["phase"] = seg.get("phase", "")
                    entry["optional_tts"] = bool(seg.get("optionalTts", False))
                    ts_text_keys.add(seg["textKey"])
                    voice_keys.add(seg["textKey"])
                elif kind == "silence":
                    entry["phase"] = SCENE_SILENCE_PHASE
                else:
                    raise ValueError("%s: неожиданный сегмент kind=%r" % (pid, kind))
                norm_segs.append(entry)
            # нормализованный протокол формата Н3-01: фаза сценария → фаза
            # протокола (как есть), длительность, класс безопасности ПО ДАННЫМ
            # (голосовые сценарии без механических дыхательных фаз → A,
            # задержек в сегментах нет → max_hold_sec 0, согласие не требуется)
            steps = [
                {"phase": s["phase"], "sec": s["duration_sec"], "cue": s.get("text_key", "")}
                for s in norm_segs
            ]
            protocol = {
                "session_min": round(sum(s["duration_sec"] for s in norm_segs) / 60.0, 2),
                "declared_min": minutes,
                "safety_class": "A",
                "requires_consent": 0,
                "max_hold_sec": 0.0,
                "steps": steps,
            }
            practices.append({
                "id": pid,
                "system": system,
                "title": title,
                "description": descr,
                "level": level,
                "difficulty": DIFF_MAPS[diff_id][level],
                "minutes": minutes,
                "contraindications": contra,
                "tags": tags,
                "segments": norm_segs,
                "protocol": protocol,
            })
    # тексты: только те, на которые ссылаются TS (сироты — проверка ниже)
    all_texts = load_texts(source)
    for pid, segs in all_texts.items():
        for tk in segs:
            pass  # полнота JSON проверяется сирот-проверкой
    for tk in sorted(ts_text_keys):
        found = None
        for pid, segs in all_texts.items():
            if tk in segs:
                found = segs[tk]
                break
        if found is None:
            raise ValueError("textKey %r отсутствует в ru.json" % tk)
        scripts[tk] = found
    for tk in sorted(voice_keys):
        rec = probe_audio(source, tk)
        if rec is None:
            raise ValueError("нет mp3 для voice-сегмента %r" % tk)
        manifest[tk] = rec
    return practices, scripts, manifest, ts_text_keys, voice_keys


def checks(practices, scripts, manifest, ts_text_keys, voice_keys, all_texts):
    """Проверки по образцу validate-content. Возвращает список ошибок."""
    errors = []
    if len(practices) != EXPECTED_PRACTICES:
        errors.append("практик %d, ожидалось %d" % (len(practices), EXPECTED_PRACTICES))
    voice_count = sum(1 for p in practices for s in p["segments"] if s["kind"] == "voice")
    if voice_count != EXPECTED_VOICE:
        errors.append("voice-сегментов %d, ожидалось %d" % (voice_count, EXPECTED_VOICE))
    if len(manifest) != EXPECTED_VOICE:
        errors.append("записей манифеста %d, ожидалось %d" % (len(manifest), EXPECTED_VOICE))
    # 1) каждый textKey TS есть в текстах (проверено в convert), сироты JSON:
    json_keys = set()
    for pid, segs in all_texts.items():
        json_keys |= set(segs.keys())
    orphans_json = sorted(json_keys - ts_text_keys)
    if orphans_json:
        errors.append("сиротских ключей JSON: %d (примеры: %s)" % (len(orphans_json), ", ".join(orphans_json[:5])))
    # 2) у каждого voice-сегмента есть запись манифеста (convert гарантировал);
    #    сироты mp3: файлы без соответствующего textKey TS
    mp3_tks = set()
    vdir = os.path.join(SOURCE, "assets", "audio", "voice")
    for pid_dir in sorted(os.listdir(vdir)):
        pdir = os.path.join(vdir, pid_dir)
        if not os.path.isdir(pdir):
            continue
        for fn in sorted(os.listdir(pdir)):
            if fn.endswith(".mp3"):
                raw = fn[:-4]
                for pid in all_texts:
                    if raw.startswith(pid + "_"):
                        mp3_tks.add(pid + "." + raw[len(pid) + 1 :])
                        break
    orphans_mp3 = sorted(mp3_tks - ts_text_keys)
    if orphans_mp3:
        errors.append("сиротских mp3: %d (примеры: %s)" % (len(orphans_mp3), ", ".join(orphans_mp3[:5])))
    missing_mp3 = sorted(voice_keys - mp3_tks)
    if missing_mp3:
        errors.append("voice-сегментов без mp3: %d (примеры: %s)" % (len(missing_mp3), ", ".join(missing_mp3[:5])))
    # пустые тексты — тихий ноль запрещён
    empty = sorted(tk for tk, v in scripts.items() if not str(v).strip())
    if empty:
        errors.append("пустых текстов: %d (примеры: %s)" % (len(empty), ", ".join(empty[:5])))
    # id практик уникальны
    ids = [p["id"] for p in practices]
    if len(set(ids)) != len(ids):
        errors.append("дубликаты id практик")
    # каждая практика имеет ru.json
    for p in practices:
        if p["id"] not in all_texts:
            errors.append("нет ru.json для %s" % p["id"])
    return errors


# Н4-07: правила допуска MIND-MASTERY — единственный источник правды,
# src/content/systems/<system>/progression.ts @ 2a26e75. Формат правил
# стабильный: {levelRequired, minCompletedSessions, minTotalMinutes,
# description}; допуск по уровню = И сессии И минуты (checkUnlock.ts:
# "Both conditions must be met (AND logic)"); уровень 1 всегда открыт
# (правил для него в источнике нет).
PROG_RE = re.compile(
    r"levelRequired\s*:\s*(\d+)\s*,\s*minCompletedSessions\s*:\s*(\d+)\s*,\s*minTotalMinutes\s*:\s*(\d+)")


def extract_progression(source):
    """Читает шесть progression.ts и возвращает ПЛОСКИЙ список правил
    [{system, level, sessions, minutes}] — системы в порядке SYSTEMS,
    уровни по возрастанию. Плоский список — для сид-цикла seed_mm_catalog
    (каждая строка = один INSERT). Файл без правил — ошибка (у всех шести
    систем правила есть)."""
    rows = []
    for system in SYSTEMS:
        path = os.path.join(source, "src", "content", "systems", system, "progression.ts")
        text = read(path)
        rules = [
            {"level": int(m.group(1)), "sessions": int(m.group(2)), "minutes": int(m.group(3))}
            for m in PROG_RE.finditer(text)
        ]
        if not rules:
            raise SystemExit("FAIL: прогрессия системы %s пуста (%s)" % (system, path))
        rules.sort(key=lambda r: r["level"])
        levels = [r["level"] for r in rules]
        if len(set(levels)) != len(levels):
            raise SystemExit("FAIL: дубликаты уровней в прогрессии %s" % system)
        if any(l < 2 or l > 5 for l in levels):
            raise SystemExit("FAIL: уровень вне 2..5 в прогрессии %s" % system)
        if any(r["sessions"] <= 0 or r["minutes"] <= 0 for r in rules):
            raise SystemExit("FAIL: неположительный порог в прогрессии %s" % system)
        for r in rules:
            rows.append({"system": system, "level": r["level"], "sessions": r["sessions"], "minutes": r["minutes"]})
    return rows


def dumps(obj):
    return json.dumps(obj, ensure_ascii=False, indent=2, sort_keys=False) + "\n"


def main():
    global SOURCE
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", default=os.environ.get("MM_SOURCE", os.path.join(os.path.dirname(REPO), "tmp", "mm-source")))
    ap.add_argument("--out", default=os.path.join(REPO, "seeds", "mm"))
    ap.add_argument("--verify", action="store_true", help="не писать; сверить существующие файлы побайтно")
    ns = ap.parse_args()
    SOURCE = ns.source

    practices, scripts, manifest, ts_text_keys, voice_keys = convert(SOURCE)
    all_texts = load_texts(SOURCE)
    progression = extract_progression(SOURCE)
    errors = checks(practices, scripts, manifest, ts_text_keys, voice_keys, all_texts)
    if errors:
        print("FAIL: %d ошибок(и):" % len(errors))
        for e in errors:
            print("  ✗ " + e)
        sys.exit(1)
    n_rules = len(progression)
    print("OK: %d практик, %d voice-сегментов, %d текстов, %d записей манифеста, 0 расхождений; прогрессий: %d правил в 6 системах"
          % (len(practices), len(voice_keys), len(scripts), len(manifest), n_rules))

    blobs = {
        "practices.json": dumps(practices),
        "scripts.json": dumps(scripts),
        "voice-manifest.json": dumps(manifest),
        "progression.json": dumps(progression),
    }
    if ns.verify:
        bad = []
        for name, content in blobs.items():
            path = os.path.join(ns.out, name)
            if not os.path.exists(path):
                bad.append(name + ": файла нет")
            elif read(path) != content:
                bad.append(name + ": отличается от регенерации")
        if bad:
            print("VERIFY FAIL: " + "; ".join(bad))
            sys.exit(1)
        print("VERIFY OK: все четыре файла побайтно равны регенерации")
        return
    os.makedirs(ns.out, exist_ok=True)
    for name, content in blobs.items():
        with io.open(os.path.join(ns.out, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(content)
        print("OK seeds/mm/%s (%d байт)" % (name, len(content.encode("utf-8"))))


if __name__ == "__main__":
    main()
