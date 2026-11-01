CREATE TABLE recon_runs (
 id INTEGER PRIMARY KEY,
 status TEXT NOT NULL CHECK(status IN ('active','complete')),
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;
CREATE UNIQUE INDEX one_active_run ON recon_runs(status) WHERE status='active';
CREATE TABLE run_operations (
 id INTEGER PRIMARY KEY,
 run_id INTEGER NOT NULL REFERENCES recon_runs(id),
 purchase_id INTEGER NOT NULL UNIQUE REFERENCES purchases(id),
 allocation TEXT NOT NULL CHECK(allocation IN ('personal','joint')),
 account_id TEXT NOT NULL,
 pot_id TEXT NOT NULL,
 amount_pence INTEGER NOT NULL CHECK(amount_pence>0),
 note TEXT NOT NULL,
 merchant TEXT NOT NULL,
 signature TEXT NOT NULL,
 snapshot TEXT NOT NULL,
 dedupe_id TEXT NOT NULL UNIQUE
) STRICT;
CREATE TABLE operation_events (
 id INTEGER PRIMARY KEY,
 operation_id INTEGER NOT NULL REFERENCES run_operations(id),
 deposit TEXT NOT NULL CHECK(deposit IN ('pending','in_flight','confirmed','failed','unknown')),
 note TEXT NOT NULL CHECK(note IN ('pending','complete')),
 message TEXT NOT NULL,
 transaction_id TEXT,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;
CREATE INDEX operation_event_index ON operation_events(operation_id,id);
CREATE UNIQUE INDEX one_transaction_operation ON operation_events(transaction_id) WHERE transaction_id IS NOT NULL;
CREATE TABLE note_mappings (
 operation_id INTEGER PRIMARY KEY REFERENCES run_operations(id),
 transaction_id TEXT NOT NULL UNIQUE,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;
CREATE TRIGGER mapping_no_update BEFORE UPDATE ON note_mappings BEGIN SELECT RAISE(ABORT,'immutable submitted note mapping'); END;
CREATE TRIGGER mapping_no_delete BEFORE DELETE ON note_mappings BEGIN SELECT RAISE(ABORT,'immutable submitted note mapping'); END;
CREATE VIEW current_operations AS SELECT o.*,e.deposit,e.note AS note_status,e.message,e.created_at AS updated_at
 FROM run_operations o JOIN operation_events e ON e.operation_id=o.id
 WHERE NOT EXISTS(SELECT 1 FROM operation_events n WHERE n.operation_id=e.operation_id AND n.id>e.id);
CREATE TRIGGER operations_no_update BEFORE UPDATE ON run_operations BEGIN SELECT RAISE(ABORT,'immutable execution instructions'); END;
CREATE TRIGGER operations_no_delete BEFORE DELETE ON run_operations BEGIN SELECT RAISE(ABORT,'immutable execution instructions'); END;
CREATE TRIGGER operation_events_no_update BEFORE UPDATE ON operation_events BEGIN SELECT RAISE(ABORT,'immutable execution results'); END;
CREATE TRIGGER operation_events_no_delete BEFORE DELETE ON operation_events BEGIN SELECT RAISE(ABORT,'immutable execution results'); END;
CREATE TRIGGER run_no_delete BEFORE DELETE ON recon_runs BEGIN SELECT RAISE(ABORT,'immutable run'); END;
CREATE TRIGGER run_status_guard BEFORE UPDATE ON recon_runs
 WHEN NEW.id!=OLD.id OR NEW.created_at!=OLD.created_at OR OLD.status!='active' OR NEW.status!='complete'
 OR EXISTS(SELECT 1 FROM current_operations WHERE run_id=OLD.id AND (deposit!='confirmed' OR note_status!='complete'))
BEGIN SELECT RAISE(ABORT,'run cannot be completed'); END;
CREATE TRIGGER deposit_transition BEFORE INSERT ON operation_events
WHEN EXISTS(SELECT 1 FROM current_operations WHERE id=NEW.operation_id AND deposit='confirmed') AND NEW.deposit!='confirmed'
BEGIN SELECT RAISE(ABORT,'confirmed deposits cannot be repeated'); END;
CREATE TRIGGER execution_revision AFTER INSERT ON operation_events BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;

-- Freeze effective instructions during a run. Imports can continue; the executor rechecks source validity.
CREATE TRIGGER executing_account BEFORE INSERT ON review_decisions
WHEN EXISTS(SELECT 1 FROM run_operations WHERE purchase_id=NEW.purchase_id)
BEGIN SELECT RAISE(ABORT,'purchase belongs to immutable execution'); END;
CREATE TRIGGER executing_details BEFORE INSERT ON purchase_details
WHEN EXISTS(SELECT 1 FROM run_operations WHERE purchase_id=NEW.purchase_id)
BEGIN SELECT RAISE(ABORT,'purchase belongs to immutable execution'); END;
CREATE TRIGGER executing_handling BEFORE INSERT ON handling_decisions
WHEN EXISTS(SELECT 1 FROM run_operations WHERE purchase_id=NEW.purchase_id)
BEGIN SELECT RAISE(ABORT,'purchase belongs to immutable execution'); END;
CREATE TRIGGER executing_staging BEFORE INSERT ON staging_events
WHEN EXISTS(SELECT 1 FROM run_operations WHERE purchase_id=NEW.purchase_id)
BEGIN SELECT RAISE(ABORT,'purchase belongs to immutable execution'); END;
