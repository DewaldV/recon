-- Versioned defaults for future runs. Tokens never belong in configuration.
CREATE TABLE destination_settings (
 version INTEGER PRIMARY KEY CHECK(version>0),
 personal_account TEXT NOT NULL,
 personal_pot TEXT NOT NULL,
 joint_account TEXT NOT NULL,
 joint_pot TEXT NOT NULL,
 saved_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 CHECK((personal_account='' AND personal_pot='') OR (personal_account!='' AND personal_pot!='')),
 CHECK((joint_account='' AND joint_pot='') OR (joint_account!='' AND joint_pot!=''))
) STRICT;
CREATE TRIGGER destinations_sequence BEFORE INSERT ON destination_settings
WHEN NEW.version != coalesce((SELECT max(version) FROM destination_settings),0)+1
BEGIN SELECT RAISE(ABORT,'nonsequential destination settings version'); END;
CREATE TRIGGER destinations_no_update BEFORE UPDATE ON destination_settings BEGIN SELECT RAISE(ABORT,'immutable destination settings'); END;
CREATE TRIGGER destinations_no_delete BEFORE DELETE ON destination_settings BEGIN SELECT RAISE(ABORT,'immutable destination settings'); END;
CREATE TRIGGER destinations_revision AFTER INSERT ON destination_settings BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
