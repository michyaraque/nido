PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
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

CREATE TABLE IF NOT EXISTS backup_targets (
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL,                     -- local | s3 | webdav
  name       TEXT NOT NULL,
  config     TEXT NOT NULL,                     -- JSON: path, or endpoint/bucket/keys, or url/user/pass
  keep       INTEGER NOT NULL DEFAULT 14,
  last_at    TEXT,
  last_ok    INTEGER,
  last_error TEXT,
  last_size  INTEGER
);

-- Payslip of `month` ticked as received on `day` (the amount it was then).
CREATE TABLE IF NOT EXISTS salary_received (
  user_id INTEGER NOT NULL REFERENCES users(id),
  month   TEXT NOT NULL,
  day     TEXT NOT NULL,
  amount  INTEGER NOT NULL,
  PRIMARY KEY (user_id, month)
);

-- One-off incomes; `received` is the day it arrived, NULL while pending.
CREATE TABLE IF NOT EXISTS incomes (
  id       INTEGER PRIMARY KEY,
  user_id  INTEGER NOT NULL REFERENCES users(id),
  month    TEXT NOT NULL,
  concept  TEXT NOT NULL,
  amount   INTEGER NOT NULL,
  received TEXT
);

-- Phones that turned on notifications (Web Push): where to send them and their keys.
CREATE TABLE IF NOT EXISTS push_subs (
  endpoint TEXT PRIMARY KEY,
  user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  p256dh   TEXT NOT NULL,
  auth     TEXT NOT NULL,
  created  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- Notifications waiting for a phone to confirm it showed them; past `due` (unix seconds) they go to Telegram.
CREATE TABLE IF NOT EXISTS push_pending (
  id      TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  html    TEXT NOT NULL,
  due     INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS push_pending_due ON push_pending(due);
CREATE INDEX IF NOT EXISTS expenses_month ON expenses(month, created);
CREATE INDEX IF NOT EXISTS settlements_month ON settlements(month);
CREATE INDEX IF NOT EXISTS shopping_pending ON shopping(cleared, done);
CREATE INDEX IF NOT EXISTS chore_logs_chore ON chore_logs(chore_id, day);
CREATE INDEX IF NOT EXISTS chore_plans_day ON chore_plans(day);

-- Atajos: an NFC tag, QR or link holds only `code`; what it does lives here and can change.
CREATE TABLE IF NOT EXISTS tags (
  id         INTEGER PRIMARY KEY,
  code       TEXT NOT NULL UNIQUE,
  name       TEXT NOT NULL,
  mode       TEXT NOT NULL DEFAULT 'all',      -- 'all': do every action | 'choose': pick one
  confirm    INTEGER NOT NULL DEFAULT 0,       -- ask before doing anything
  created_by INTEGER REFERENCES users(id),
  created    TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- What a tag does, in order. `label` and `icon` are kept so the tag shows without asking a plugin.
CREATE TABLE IF NOT EXISTS tag_actions (
  tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  pos     INTEGER NOT NULL,
  source  TEXT NOT NULL,                       -- 'core' | plugin id
  action  TEXT NOT NULL,                       -- the action id its source gave, e.g. 'meal:3'
  arg     TEXT NOT NULL DEFAULT '',            -- what the person wrote for it ("Leche")
  label   TEXT NOT NULL,
  icon    TEXT NOT NULL DEFAULT 'zap',
  confirm INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (tag_id, pos)
);

-- Each scan: who, when, what happened (JSON lines for its page) and how to take it back
-- (JSON [[source, token], ...]).
CREATE TABLE IF NOT EXISTS tag_runs (
  id      INTEGER PRIMARY KEY,
  tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  user_id INTEGER NOT NULL REFERENCES users(id),
  at      TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
  result  TEXT NOT NULL DEFAULT '[]',
  undo    TEXT NOT NULL DEFAULT '[]',
  undone  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS tag_runs_tag ON tag_runs(tag_id, at);
CREATE INDEX IF NOT EXISTS tag_runs_at ON tag_runs(at);
