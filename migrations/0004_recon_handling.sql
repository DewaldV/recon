-- Handling is global purchase history; recon membership is a separate selection.
CREATE TABLE handling_decisions (
 id INTEGER PRIMARY KEY,
 purchase_id INTEGER NOT NULL REFERENCES purchases(id),
 version INTEGER NOT NULL CHECK(version > 0),
 state TEXT NOT NULL CHECK(state IN ('unknown','outstanding','external','excluded')),
 reason TEXT NOT NULL CHECK(length(trim(reason)) BETWEEN 1 AND 4000),
 reopen_confirmed INTEGER NOT NULL CHECK(reopen_confirmed IN (0,1)),
 provenance TEXT NOT NULL CHECK(provenance='manual'),
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 UNIQUE(purchase_id,version)
) STRICT;
CREATE VIEW current_handling AS SELECT h.* FROM handling_decisions h WHERE NOT EXISTS(
 SELECT 1 FROM handling_decisions n WHERE n.purchase_id=h.purchase_id AND n.version>h.version);
CREATE TRIGGER handling_sequence BEFORE INSERT ON handling_decisions
WHEN NEW.version != coalesce((SELECT max(version) FROM handling_decisions WHERE purchase_id=NEW.purchase_id),0)+1
BEGIN SELECT RAISE(ABORT,'nonsequential handling version'); END;
CREATE TRIGGER handling_reopen BEFORE INSERT ON handling_decisions
WHEN NEW.state='outstanding' AND NEW.reopen_confirmed=0 AND EXISTS(
 SELECT 1 FROM handling_decisions WHERE purchase_id=NEW.purchase_id AND state IN ('external','excluded'))
BEGIN SELECT RAISE(ABORT,'reopening handled work requires confirmation'); END;
CREATE TRIGGER handling_no_update BEFORE UPDATE ON handling_decisions BEGIN SELECT RAISE(ABORT,'append-only handling history'); END;
CREATE TRIGGER handling_no_delete BEFORE DELETE ON handling_decisions BEGIN SELECT RAISE(ABORT,'append-only handling history'); END;
CREATE TRIGGER handling_revision AFTER INSERT ON handling_decisions BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;

CREATE TABLE recons (
 id INTEGER PRIMARY KEY,
 name TEXT NOT NULL CHECK(length(trim(name)) BETWEEN 1 AND 200),
 name_key TEXT NOT NULL UNIQUE,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;
CREATE TABLE recon_status_events (
 id INTEGER PRIMARY KEY,
 recon_id INTEGER NOT NULL REFERENCES recons(id),
 version INTEGER NOT NULL CHECK(version>0),
 status TEXT NOT NULL CHECK(status IN ('active','archived')),
 reason TEXT NOT NULL CHECK(length(trim(reason)) BETWEEN 1 AND 4000),
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 UNIQUE(recon_id,version)
) STRICT;
CREATE VIEW current_recon_status AS SELECT s.* FROM recon_status_events s WHERE NOT EXISTS(
 SELECT 1 FROM recon_status_events n WHERE n.recon_id=s.recon_id AND n.version>s.version);
CREATE TABLE recon_membership_events (
 id INTEGER PRIMARY KEY,
 recon_id INTEGER NOT NULL REFERENCES recons(id),
 purchase_id INTEGER NOT NULL REFERENCES purchases(id),
 version INTEGER NOT NULL CHECK(version>0),
 action TEXT NOT NULL CHECK(action IN ('add','remove')),
 reason TEXT NOT NULL CHECK(length(trim(reason)) BETWEEN 1 AND 4000),
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 UNIQUE(recon_id,purchase_id,version)
) STRICT;
CREATE VIEW current_recon_memberships AS SELECT m.* FROM recon_membership_events m WHERE m.action='add' AND NOT EXISTS(
 SELECT 1 FROM recon_membership_events n WHERE n.recon_id=m.recon_id AND n.purchase_id=m.purchase_id AND n.version>m.version);
CREATE INDEX membership_purchase ON recon_membership_events(purchase_id);
CREATE TRIGGER membership_sequence BEFORE INSERT ON recon_membership_events
WHEN NEW.version != coalesce((SELECT max(version) FROM recon_membership_events WHERE recon_id=NEW.recon_id AND purchase_id=NEW.purchase_id),0)+1
BEGIN SELECT RAISE(ABORT,'nonsequential membership version'); END;
CREATE TRIGGER membership_active BEFORE INSERT ON recon_membership_events
WHEN NOT EXISTS(SELECT 1 FROM current_recon_status WHERE recon_id=NEW.recon_id AND status='active')
BEGIN SELECT RAISE(ABORT,'cannot edit archived recon membership'); END;
CREATE TRIGGER membership_unique_active BEFORE INSERT ON recon_membership_events
WHEN NEW.action='add' AND EXISTS(SELECT 1 FROM current_recon_memberships m JOIN current_recon_status s ON s.recon_id=m.recon_id
 WHERE m.purchase_id=NEW.purchase_id AND m.recon_id!=NEW.recon_id AND s.status='active')
BEGIN SELECT RAISE(ABORT,'purchase already in another active recon'); END;
CREATE TRIGGER status_sequence BEFORE INSERT ON recon_status_events
WHEN NEW.version != coalesce((SELECT max(version) FROM recon_status_events WHERE recon_id=NEW.recon_id),0)+1
BEGIN SELECT RAISE(ABORT,'nonsequential recon status version'); END;
CREATE TRIGGER status_unique_active BEFORE INSERT ON recon_status_events
WHEN NEW.status='active' AND EXISTS(
 SELECT 1 FROM current_recon_memberships own JOIN current_recon_memberships other ON other.purchase_id=own.purchase_id
 JOIN current_recon_status s ON s.recon_id=other.recon_id
 WHERE own.recon_id=NEW.recon_id AND other.recon_id!=NEW.recon_id AND s.status='active')
BEGIN SELECT RAISE(ABORT,'reactivation would duplicate active membership'); END;
CREATE TRIGGER recons_no_update BEFORE UPDATE ON recons BEGIN SELECT RAISE(ABORT,'immutable recon identity'); END;
CREATE TRIGGER recons_no_delete BEFORE DELETE ON recons BEGIN SELECT RAISE(ABORT,'immutable recon identity'); END;
CREATE TRIGGER recon_status_no_update BEFORE UPDATE ON recon_status_events BEGIN SELECT RAISE(ABORT,'append-only recon history'); END;
CREATE TRIGGER recon_status_no_delete BEFORE DELETE ON recon_status_events BEGIN SELECT RAISE(ABORT,'append-only recon history'); END;
CREATE TRIGGER recon_membership_no_update BEFORE UPDATE ON recon_membership_events BEGIN SELECT RAISE(ABORT,'append-only membership history'); END;
CREATE TRIGGER recon_membership_no_delete BEFORE DELETE ON recon_membership_events BEGIN SELECT RAISE(ABORT,'append-only membership history'); END;
CREATE TRIGGER recons_revision AFTER INSERT ON recons BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER recon_status_revision AFTER INSERT ON recon_status_events BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER recon_membership_revision AFTER INSERT ON recon_membership_events BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;
