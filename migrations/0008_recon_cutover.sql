-- A fixed migration boundary, not a rolling last-run watermark.
CREATE TABLE recon_cutover (
 id INTEGER PRIMARY KEY CHECK(id=1),
 posting_date TEXT NOT NULL CHECK(length(posting_date)=10),
 reason TEXT NOT NULL CHECK(length(trim(reason)) BETWEEN 1 AND 3500),
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;
CREATE TRIGGER cutover_no_update BEFORE UPDATE ON recon_cutover BEGIN SELECT RAISE(ABORT,'immutable recon cutover'); END;
CREATE TRIGGER cutover_no_delete BEFORE DELETE ON recon_cutover BEGIN SELECT RAISE(ABORT,'immutable recon cutover'); END;
CREATE TRIGGER cutover_revision AFTER INSERT ON recon_cutover BEGIN UPDATE review_revision SET revision=revision+1 WHERE id=1; END;

-- Even an explicitly confirmed reopening cannot put pre-cutover work into a run.
CREATE TRIGGER handling_cutover BEFORE INSERT ON handling_decisions
WHEN NEW.state IN ('unknown','outstanding') AND EXISTS (
 SELECT 1 FROM purchases p JOIN recon_cutover c ON p.posting_date<c.posting_date WHERE p.id=NEW.purchase_id
)
BEGIN SELECT RAISE(ABORT,'purchase predates the recon start date'); END;
CREATE TRIGGER operation_cutover BEFORE INSERT ON run_operations
WHEN EXISTS (
 SELECT 1 FROM purchases p JOIN recon_cutover c ON p.posting_date<c.posting_date WHERE p.id=NEW.purchase_id
)
BEGIN SELECT RAISE(ABORT,'purchase predates the recon start date'); END;
