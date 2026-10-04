-- Kamerton schema v1: the six tables of the trial engine (concept v2,
-- section "Trial, data, engine"). All timestamps are ISO 8601 UTC strings
-- so any third-party SQLite client can read the data without this app
-- (AGENTS.md invariant 6).
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS note (
    id         INTEGER PRIMARY KEY,
    text       TEXT    NOT NULL CHECK (length(text) > 0),
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS arm (
    id           INTEGER PRIMARY KEY,
    method       TEXT    NOT NULL,
    duration_min INTEGER NOT NULL CHECK (duration_min > 0),
    day_window   TEXT    NOT NULL CHECK (day_window IN ('morning', 'evening')),
    prior_mean   REAL    NOT NULL CHECK (prior_mean >= 0 AND prior_mean <= 1),
    prior_sd     REAL    NOT NULL CHECK (prior_sd > 0),
    prior_source TEXT    NOT NULL CHECK (length(prior_source) > 0),
    is_active    INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at   TEXT    NOT NULL,
    UNIQUE (method, duration_min, day_window)
);

CREATE TABLE IF NOT EXISTS trial (
    id             INTEGER PRIMARY KEY,
    note_id        INTEGER NOT NULL REFERENCES note(id),
    arm_id         INTEGER NOT NULL REFERENCES arm(id),
    started_at     TEXT    NOT NULL,
    predicted      TEXT    NOT NULL,
    selection_prob REAL    NOT NULL CHECK (selection_prob > 0 AND selection_prob <= 1),
    is_exploration INTEGER NOT NULL CHECK (is_exploration IN (0, 1))
);

CREATE INDEX IF NOT EXISTS idx_trial_arm   ON trial(arm_id);
CREATE INDEX IF NOT EXISTS idx_trial_start ON trial(started_at);

CREATE TABLE IF NOT EXISTS response (
    id        INTEGER PRIMARY KEY,
    trial_id  INTEGER NOT NULL REFERENCES trial(id),
    kind      TEXT    NOT NULL CHECK (kind IN ('immediate', 'shift24h', 'action72h')),
    score     REAL    NOT NULL CHECK (score >= 0 AND score <= 1),
    logged_at TEXT    NOT NULL,
    UNIQUE (trial_id, kind)
);

CREATE INDEX IF NOT EXISTS idx_response_trial ON response(trial_id);

CREATE TABLE IF NOT EXISTS context (
    id            INTEGER PRIMARY KEY,
    trial_id      INTEGER NOT NULL REFERENCES trial(id),
    logged_at     TEXT    NOT NULL,
    day_window    TEXT    NOT NULL CHECK (day_window IN ('morning', 'evening')),
    sleep_quality INTEGER CHECK (sleep_quality BETWEEN 1 AND 5),
    moon_phase    REAL    CHECK (moon_phase >= 0 AND moon_phase < 1),
    moon_day      INTEGER CHECK (moon_day >= 1 AND moon_day <= 30),
    revealed_at   TEXT,
    UNIQUE (trial_id)
);

CREATE INDEX IF NOT EXISTS idx_context_trial ON context(trial_id);

CREATE TABLE IF NOT EXISTS candidate (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL UNIQUE,
    source     TEXT    NOT NULL CHECK (source IN ('time', 'sleep', 'moon', 'weather', 'other')),
    status     TEXT    NOT NULL DEFAULT 'candidate' CHECK (status IN ('candidate', 'promoted', 'rejected')),
    evidence   TEXT    NOT NULL DEFAULT '',
    decided_at TEXT
);
