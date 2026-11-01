CREATE TABLE review_revision (id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL) STRICT;
INSERT INTO review_revision VALUES (1,0);
CREATE TABLE categories (name TEXT PRIMARY KEY CHECK(length(trim(name))>0), normalized TEXT NOT NULL UNIQUE) STRICT;
CREATE TABLE merchant_sources (
 id INTEGER PRIMARY KEY, sha256 TEXT NOT NULL UNIQUE, filename TEXT NOT NULL,
 bytes BLOB NOT NULL, imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;
CREATE TABLE merchant_definitions (
 id INTEGER PRIMARY KEY, merchant_key TEXT NOT NULL, original_name TEXT NOT NULL,
 version INTEGER NOT NULL, category TEXT REFERENCES categories(name), description TEXT,
 source_id INTEGER REFERENCES merchant_sources(id), source_record INTEGER,
 normalization_version INTEGER NOT NULL DEFAULT 1 CHECK(normalization_version=1),
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, UNIQUE(merchant_key,version)
) STRICT;
CREATE VIEW current_merchants AS SELECT d.* FROM merchant_definitions d
WHERE NOT EXISTS(SELECT 1 FROM merchant_definitions n WHERE n.merchant_key=d.merchant_key AND n.version>d.version);
CREATE TABLE purchase_details (
 id INTEGER PRIMARY KEY, purchase_id INTEGER NOT NULL REFERENCES purchases(id), version INTEGER NOT NULL,
 category TEXT REFERENCES categories(name), description TEXT,
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, UNIQUE(purchase_id,version)
) STRICT;
CREATE VIEW current_details AS SELECT d.* FROM purchase_details d WHERE NOT EXISTS(
 SELECT 1 FROM purchase_details n WHERE n.purchase_id=d.purchase_id AND n.version>d.version);
CREATE TABLE merchant_aliases (
 alias_key TEXT PRIMARY KEY, target_key TEXT NOT NULL, original_name TEXT NOT NULL,
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, CHECK(alias_key!=target_key)
) STRICT;
CREATE TABLE review_previews (
 token TEXT PRIMARY KEY, revision INTEGER NOT NULL, payload TEXT NOT NULL,
 committed INTEGER NOT NULL DEFAULT 0 CHECK(committed IN (0,1)), committed_at TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;
CREATE TRIGGER merchants_sequence BEFORE INSERT ON merchant_definitions WHEN NEW.version != coalesce((SELECT max(version) FROM merchant_definitions WHERE merchant_key=NEW.merchant_key),0)+1
BEGIN SELECT RAISE(ABORT,'nonsequential merchant version'); END;
CREATE TRIGGER details_sequence BEFORE INSERT ON purchase_details WHEN NEW.version != coalesce((SELECT max(version) FROM purchase_details WHERE purchase_id=NEW.purchase_id),0)+1
BEGIN SELECT RAISE(ABORT,'nonsequential details version'); END;
CREATE TRIGGER merchant_sources_no_update BEFORE UPDATE ON merchant_sources BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER merchant_sources_no_delete BEFORE DELETE ON merchant_sources BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER merchant_definitions_no_update BEFORE UPDATE ON merchant_definitions BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER merchant_definitions_no_delete BEFORE DELETE ON merchant_definitions BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER purchase_details_no_update BEFORE UPDATE ON purchase_details BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER purchase_details_no_delete BEFORE DELETE ON purchase_details BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER merchant_aliases_no_update BEFORE UPDATE ON merchant_aliases BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER merchant_aliases_no_delete BEFORE DELETE ON merchant_aliases BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER categories_no_update BEFORE UPDATE ON categories BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER categories_no_delete BEFORE DELETE ON categories BEGIN SELECT RAISE(ABORT,'append-only review data'); END;
CREATE TRIGGER merchant_definitions_revision AFTER INSERT ON merchant_definitions BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER purchase_details_revision AFTER INSERT ON purchase_details BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER merchant_aliases_revision AFTER INSERT ON merchant_aliases BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER categories_revision AFTER INSERT ON categories BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER review_decisions_revision AFTER INSERT ON review_decisions BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER source_observations_revision AFTER INSERT ON source_observations BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER previews_no_rewrite BEFORE UPDATE ON review_previews
WHEN NEW.token!=OLD.token OR NEW.revision!=OLD.revision OR NEW.payload!=OLD.payload OR OLD.committed=1 OR NEW.committed!=1
BEGIN SELECT RAISE(ABORT,'preview content is immutable'); END;
CREATE TRIGGER previews_no_delete BEFORE DELETE ON review_previews
BEGIN SELECT RAISE(ABORT,'preview audit is immutable'); END;
