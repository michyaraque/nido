PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
  id     INTEGER PRIMARY KEY,
  name   TEXT NOT NULL UNIQUE,
  pass   TEXT NOT NULL,
  salary INTEGER NOT NULL DEFAULT 0,          -- cents
  token  TEXT NOT NULL UNIQUE                 -- API token (iPhone Shortcut)
);

CREATE TABLE IF NOT EXISTS sessions (
  token   TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE
);

-- Monthly bills. owner_id NULL = shared by the household, split by salary.
CREATE TABLE IF NOT EXISTS bills (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  amount      INTEGER NOT NULL,
  due_day     INTEGER NOT NULL CHECK (due_day BETWEEN 1 AND 31),
  owner_id    INTEGER REFERENCES users(id),
  start_month TEXT NOT NULL,                  -- 'YYYY-MM', inclusive
  end_month   TEXT                            -- 'YYYY-MM', exclusive (ending a bill keeps its history)
);

CREATE TABLE IF NOT EXISTS payments (
  bill_id  INTEGER NOT NULL REFERENCES bills(id),
  month    TEXT NOT NULL,
  payer_id INTEGER NOT NULL REFERENCES users(id),
  PRIMARY KEY (bill_id, month)
);

CREATE TABLE IF NOT EXISTS expenses (
  id          INTEGER PRIMARY KEY,
  user_id     INTEGER NOT NULL REFERENCES users(id),
  month       TEXT NOT NULL,
  description TEXT NOT NULL,
  amount      INTEGER NOT NULL,
  source      TEXT NOT NULL DEFAULT 'web',    -- 'web' | 'shortcut'
  created     TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- One-off amount for a single month (e.g. this month's electricity).
CREATE TABLE IF NOT EXISTS bill_amounts (
  bill_id INTEGER NOT NULL REFERENCES bills(id),
  month   TEXT NOT NULL,
  amount  INTEGER NOT NULL,
  PRIMARY KEY (bill_id, month)
);

-- Money paid back between partners to even out shared bills.
CREATE TABLE IF NOT EXISTS settlements (
  id      INTEGER PRIMARY KEY,
  month   TEXT NOT NULL,
  from_id INTEGER NOT NULL REFERENCES users(id),
  to_id   INTEGER NOT NULL REFERENCES users(id),
  amount  INTEGER NOT NULL,
  created TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- Receipt photos. The image lives in <data dir>/tickets/<id>.jpg.
CREATE TABLE IF NOT EXISTS tickets (
  id         INTEGER PRIMARY KEY,
  user_id    INTEGER NOT NULL REFERENCES users(id),
  status     TEXT NOT NULL DEFAULT 'reading',   -- reading | ready | failed | done
  merchant   TEXT,
  date       TEXT,                              -- 'YYYY-MM-DD'
  total      INTEGER,                           -- cents
  items      TEXT,                              -- JSON [[name, cents], ...]
  text       TEXT,                              -- raw OCR text
  error      TEXT,
  expense_id INTEGER REFERENCES expenses(id) ON DELETE SET NULL,
  created    TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- Shared shopping list. Cleared items stay as history for suggestions.
CREATE TABLE IF NOT EXISTS shopping (
  id       INTEGER PRIMARY KEY,
  name     TEXT NOT NULL,
  done     INTEGER NOT NULL DEFAULT 0,
  cleared  INTEGER NOT NULL DEFAULT 0,
  added_by INTEGER REFERENCES users(id),
  created  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- Household chores defined by the users (name, icon, colour, how often they should happen).
CREATE TABLE IF NOT EXISTS chores (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  icon       TEXT NOT NULL,
  color      TEXT NOT NULL,
  every_days INTEGER,                           -- NULL = no schedule
  archived   INTEGER NOT NULL DEFAULT 0,
  created    TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- Who did which chore, and on which day.
CREATE TABLE IF NOT EXISTS chore_logs (
  id       INTEGER PRIMARY KEY,
  chore_id INTEGER NOT NULL REFERENCES chores(id) ON DELETE CASCADE,
  user_id  INTEGER NOT NULL REFERENCES users(id),
  day      TEXT NOT NULL,                       -- 'YYYY-MM-DD'
  created  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);
CREATE INDEX IF NOT EXISTS chore_logs_day ON chore_logs(day);

-- A chore planned for a day: for someone in particular, or for whoever takes it.
CREATE TABLE IF NOT EXISTS chore_plans (
  id          INTEGER PRIMARY KEY,
  chore_id    INTEGER NOT NULL REFERENCES chores(id) ON DELETE CASCADE,
  day         TEXT NOT NULL,                    -- 'YYYY-MM-DD'
  assignee_id INTEGER REFERENCES users(id),     -- NULL = anyone can take it
  log_id      INTEGER REFERENCES chore_logs(id) ON DELETE SET NULL, -- set once done
  created     TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- Household-wide preferences (key → value).
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
