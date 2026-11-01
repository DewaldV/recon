CREATE TABLE historical_sources (
 id INTEGER PRIMARY KEY,
 sha256 TEXT NOT NULL UNIQUE,
 filename TEXT NOT NULL,
 bytes BLOB NOT NULL,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;
CREATE TABLE historical_matches (
 source_id INTEGER NOT NULL REFERENCES historical_sources(id),
 record INTEGER NOT NULL,
 purchase_id INTEGER NOT NULL REFERENCES purchases(id),
 proposed TEXT NOT NULL,
 previous_snapshot TEXT NOT NULL,
 PRIMARY KEY(source_id,record),
 UNIQUE(source_id,purchase_id)
) STRICT;
CREATE TRIGGER historical_sources_no_update BEFORE UPDATE ON historical_sources BEGIN SELECT RAISE(ABORT,'immutable historical provenance'); END;
CREATE TRIGGER historical_sources_no_delete BEFORE DELETE ON historical_sources BEGIN SELECT RAISE(ABORT,'immutable historical provenance'); END;
CREATE TRIGGER historical_matches_no_update BEFORE UPDATE ON historical_matches BEGIN SELECT RAISE(ABORT,'immutable historical match'); END;
CREATE TRIGGER historical_matches_no_delete BEFORE DELETE ON historical_matches BEGIN SELECT RAISE(ABORT,'immutable historical match'); END;
CREATE TRIGGER historical_revision AFTER INSERT ON historical_matches BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
