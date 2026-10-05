# Migrations — the operator side of the Kamerton schema

The schema lives twice, on purpose:

1. **The language path** — the `schema kamerton { ... }` block in
   `src/store/store.mlog`. `mlog serve` / `mlog run` replay it on every
   connection open: additive-only `CREATE TABLE IF NOT EXISTS`
   (ADR-0060, №593). This is what the app does by itself.
2. **The operator path** — `0001_init.sql`, applied by a plain sqlite3
   client. This is the path the acceptance criterion calls for:
   `sqlite3` must be able to read and maintain the schema without the
   app (invariant 6), and it is the recovery path of ADR-0183.

Both are idempotent: re-applying changes nothing.

## How to apply

```bash
sqlite3 /data/kamerton/kamerton.db < migrations/0001_init.sql
```

## Rollback (ADR-0183, variant B)

There is no down-migration, in the language or here — the destructive
DDL class is deliberately outside the project. The rollback of a bad
migration is **backup restore**: take the last verified copy from the
weekly backup (`deploy/backup.sh`, restore-checked on every run) and
replay the migrations that are still current. The 0001 file itself is
safe to re-apply after a restore.

## Known delta vs the language replay (v0.28.1)

The mlog v0.28.1 schema-as-code parser silently drops the
`references(t.f)` modifier (the compact form loses the modifier, the
spaced form produces invalid SQL — declared loudly per AGENTS §2 with
a minimal reproducer in the naryad worklog). Consequences until the
language fix lands:

- a database created ONLY by the language replay lacks the FK clauses;
- this mirror carries them;
- the store's tests pin the mirror (`foreign keys: the operator mirror
  carries them`) so the delta cannot disappear silently either way.

## The M0 import CSV (fixed structure)

The manual baseline (M0) imports through the store's `import_m0`:

```
date,window,depth,sleep_quality,free_note
2026-10-01,morning,0.6,4.0,calm baseline
```

One row per проба, header first, five columns; `free_note` is the last
column and may contain commas; `depth` and `sleep_quality` are numeric
(the import fails loudly on garbage). The import lands each row as a
`trial` with status `imported`, a `moment` response with the
`subjective` source, and a `context` row carrying the sleep quality —
the same rows the direct input path produces (tested).
