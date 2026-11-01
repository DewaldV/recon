-- Supersedes workspace membership for new work. Old selections remain audit history.
CREATE TABLE account_rules (
 id INTEGER PRIMARY KEY,
 merchant_key TEXT NOT NULL,
 merchant_name TEXT NOT NULL,
 card TEXT NOT NULL DEFAULT '',
 version INTEGER NOT NULL CHECK(version > 0),
 account TEXT CHECK(account IN ('personal','joint')),
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 UNIQUE(merchant_key,card,version)
) STRICT;
CREATE VIEW current_account_rules AS SELECT r.* FROM account_rules r WHERE NOT EXISTS (
 SELECT 1 FROM account_rules n WHERE n.merchant_key=r.merchant_key AND n.card=r.card AND n.version>r.version
);
CREATE TRIGGER account_rules_sequence BEFORE INSERT ON account_rules
WHEN NEW.version != coalesce((SELECT max(version) FROM account_rules WHERE merchant_key=NEW.merchant_key AND card=NEW.card),0)+1
BEGIN SELECT RAISE(ABORT,'nonsequential rule version'); END;
CREATE TRIGGER account_rules_no_update BEFORE UPDATE ON account_rules BEGIN SELECT RAISE(ABORT,'immutable account rule'); END;
CREATE TRIGGER account_rules_no_delete BEFORE DELETE ON account_rules BEGIN SELECT RAISE(ABORT,'immutable account rule'); END;
CREATE TRIGGER account_rules_revision AFTER INSERT ON account_rules BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;

CREATE TABLE staging_events (
 id INTEGER PRIMARY KEY,
 purchase_id INTEGER NOT NULL REFERENCES purchases(id),
 staged INTEGER NOT NULL CHECK(staged IN (0,1)),
 signature TEXT NOT NULL,
 snapshot TEXT NOT NULL,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;
CREATE INDEX staging_purchase ON staging_events(purchase_id,id);
CREATE VIEW current_staging AS SELECT s.* FROM staging_events s WHERE NOT EXISTS (
 SELECT 1 FROM staging_events n WHERE n.purchase_id=s.purchase_id AND n.id>s.id
);
CREATE TRIGGER staging_no_update BEFORE UPDATE ON staging_events BEGIN SELECT RAISE(ABORT,'immutable staging event'); END;
CREATE TRIGGER staging_no_delete BEFORE DELETE ON staging_events BEGIN SELECT RAISE(ABORT,'immutable staging event'); END;
CREATE TRIGGER staging_revision AFTER INSERT ON staging_events BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;

-- A preview's return path is server-validated, persisted, and never a browser-supplied redirect at commit.
CREATE TABLE preview_context (
 token TEXT PRIMARY KEY REFERENCES review_previews(token),
 return_to TEXT NOT NULL
) STRICT;

-- Explicit dispositions for raw observations which cannot enter the canonical purchase queue.
CREATE TABLE exception_decisions (
 id INTEGER PRIMARY KEY,
 observation_id INTEGER NOT NULL REFERENCES source_observations(id),
 reason TEXT NOT NULL CHECK(length(trim(reason)) BETWEEN 1 AND 4000),
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 UNIQUE(observation_id)
) STRICT;
CREATE TRIGGER exception_no_update BEFORE UPDATE ON exception_decisions BEGIN SELECT RAISE(ABORT,'immutable exception decision'); END;
CREATE TRIGGER exception_no_delete BEFORE DELETE ON exception_decisions BEGIN SELECT RAISE(ABORT,'immutable exception decision'); END;
CREATE TRIGGER exception_revision AFTER INSERT ON exception_decisions BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;

-- Old preview links must not recreate workspace actions after upgrade.
UPDATE review_revision SET revision=revision+1 WHERE id=1;
