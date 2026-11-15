-- Recetas (plugin). Its own database, recipes.db: Nido's database never holds a recipe.
CREATE TABLE IF NOT EXISTS recipes (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  category    TEXT NOT NULL DEFAULT 'Otros',
  area        TEXT NOT NULL DEFAULT '',          -- where it comes from: "Asturias", "Italia"…
  serves      INTEGER NOT NULL DEFAULT 2 CHECK (serves BETWEEN 1 AND 50),
  minutes     INTEGER,                           -- total time, optional
  note        TEXT NOT NULL DEFAULT '',
  source      TEXT NOT NULL DEFAULT '',          -- a web page, "themealdb:52772" or "nido:tortilla-de-patatas" (built-in catalog)
  photo       INTEGER NOT NULL DEFAULT 0,        -- 0 none · 1 built-in catalog photo · else version of its photo in recipe_photos
  favorite    INTEGER NOT NULL DEFAULT 0,
  cooked      INTEGER NOT NULL DEFAULT 0,        -- times the house has made it
  added_by    INTEGER,                           -- id of the person in Nido (X-Nido-User)
  created     TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
  search      TEXT NOT NULL DEFAULT ''           -- name, origin and category without accents, for search
);
CREATE INDEX IF NOT EXISTS recipes_source ON recipes(source);

-- One row per ingredient line, in order. A section ("Para la salsa") is a title, not something to buy.
CREATE TABLE IF NOT EXISTS recipe_ingredients (
  recipe_id INTEGER NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
  pos       INTEGER NOT NULL,
  section   INTEGER NOT NULL DEFAULT 0,
  text      TEXT NOT NULL,                       -- as written: "200 g de harina"
  item      TEXT NOT NULL DEFAULT '',            -- the product, without accents: "harina" (search, Compra)
  PRIMARY KEY (recipe_id, pos)
) WITHOUT ROWID;

-- One row per step, in order.
CREATE TABLE IF NOT EXISTS recipe_steps (
  recipe_id INTEGER NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
  pos       INTEGER NOT NULL,
  kind      TEXT NOT NULL DEFAULT '',            -- '' step · 'h' section title · 'tip' a tip
  text      TEXT NOT NULL,
  PRIMARY KEY (recipe_id, pos)
) WITHOUT ROWID;

-- Uploaded and imported photos, apart so listing recipes never reads image bytes. Inside the
-- database of the plugin. Catalog photos land here too, only if the house downloads them.
CREATE TABLE IF NOT EXISTS recipe_photos (
  recipe_id INTEGER PRIMARY KEY REFERENCES recipes(id) ON DELETE CASCADE,
  webp      BLOB NOT NULL
);

-- Catalog recipes the house deleted: new releases do not bring them back.
CREATE TABLE IF NOT EXISTS recipe_hidden (
  source TEXT PRIMARY KEY
);

-- Small preferences of the plugin (like "photos_skip": the house said no to the catalog photos).
CREATE TABLE IF NOT EXISTS recipe_settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
