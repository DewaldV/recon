-- Append repair provenance without rewriting archived observations.
-- For new imports both IDs are the same. Recovery of a previously quarantined
-- row points to its replacement in an archived, generated corrected import.
CREATE TABLE observation_repairs (
 observation_id INTEGER PRIMARY KEY REFERENCES source_observations(id),
 replacement_observation_id INTEGER NOT NULL REFERENCES source_observations(id),
 method TEXT NOT NULL CHECK(length(method)>0),
 normalized_record TEXT NOT NULL,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;
CREATE TRIGGER repairs_same_source BEFORE INSERT ON observation_repairs
WHEN (SELECT source_account_id FROM source_observations WHERE id=NEW.observation_id)
 != (SELECT source_account_id FROM source_observations WHERE id=NEW.replacement_observation_id)
BEGIN SELECT RAISE(ABORT,'repair crosses source accounts'); END;
CREATE TRIGGER repairs_no_update BEFORE UPDATE ON observation_repairs BEGIN SELECT RAISE(ABORT,'immutable import repair'); END;
CREATE TRIGGER repairs_no_delete BEFORE DELETE ON observation_repairs BEGIN SELECT RAISE(ABORT,'immutable import repair'); END;
CREATE TRIGGER repairs_revision AFTER INSERT ON observation_repairs BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
