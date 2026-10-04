-- Rollback of 0001_init.sql. Drops the six tables in reverse dependency
-- order. Idempotent: safe to run on a fresh database.
PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS candidate;
DROP TABLE IF EXISTS context;
DROP TABLE IF EXISTS response;
DROP TABLE IF EXISTS trial;
DROP TABLE IF EXISTS arm;
DROP TABLE IF EXISTS note;

PRAGMA foreign_keys = ON;
