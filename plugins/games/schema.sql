-- Juegos (extra). Its own database: Nido's database never holds a match.
CREATE TABLE IF NOT EXISTS matches (
  id         INTEGER PRIMARY KEY,
  game       TEXT NOT NULL,                     -- ttt | c4 | fleet | oca | parchis | colores
  players    TEXT NOT NULL,                     -- JSON: Nido user ids in seat order, 0 = robot
  state      TEXT NOT NULL,                     -- JSON of the game
  turn       INTEGER NOT NULL DEFAULT 0,        -- seat to play (for the lists)
  done       INTEGER NOT NULL DEFAULT 0,
  winner     INTEGER,                           -- seat; done with no winner = a draw
  note       TEXT NOT NULL DEFAULT '',          -- how it ended, if not by playing ("Ana ha abandonado")
  version    INTEGER NOT NULL DEFAULT 1,        -- grows on every move: phones poll it to refresh
  created_by INTEGER,
  created    TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
  updated    TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);
CREATE INDEX IF NOT EXISTS matches_recent ON matches(done, updated);

-- One row per person per finished match: the ranking is a sum over this.
CREATE TABLE IF NOT EXISTS results (
  match_id INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
  user_id  INTEGER NOT NULL,
  game     TEXT NOT NULL,
  outcome  TEXT NOT NULL,                       -- win | draw | loss
  points   INTEGER NOT NULL,                    -- 3 win, 1 draw, 0 loss
  at       TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
  PRIMARY KEY (match_id, user_id)
);

-- Emoji reactions during a match (shown for a few seconds next to the player).
CREATE TABLE IF NOT EXISTS reactions (
  match_id INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
  seat     INTEGER NOT NULL,
  emoji    TEXT NOT NULL,
  at       INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);
CREATE INDEX IF NOT EXISTS reactions_match ON reactions(match_id, at);

-- Invitations for people of the house: Nido shows them in the open app (GET /notices) and, if
-- nobody saw them after a minute, sends them by Telegram (GET /notices/late).
CREATE TABLE IF NOT EXISTS notices (
  id       INTEGER PRIMARY KEY,
  user_id  INTEGER NOT NULL,
  match_id INTEGER NOT NULL,
  text     TEXT NOT NULL,
  at       INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
  seen     INTEGER NOT NULL DEFAULT 0,
  sent     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS notices_user ON notices(user_id, seen);
