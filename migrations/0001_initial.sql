CREATE TABLE source_accounts (
    id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL CHECK (provider = 'virgin'),
    account_key TEXT NOT NULL UNIQUE CHECK (length(trim(account_key)) > 0)
) STRICT;

CREATE TABLE imports (
    id INTEGER PRIMARY KEY,
    source_account_id INTEGER NOT NULL REFERENCES source_accounts(id),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    original_filename TEXT NOT NULL,
    original_bytes BLOB NOT NULL,
    imported_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (source_account_id, sha256),
    UNIQUE (id, source_account_id)
) STRICT;

CREATE TABLE purchases (
    id INTEGER PRIMARY KEY,
    source_account_id INTEGER NOT NULL REFERENCES source_accounts(id),
    reference TEXT NOT NULL CHECK (length(trim(reference)) > 0 AND lower(trim(reference)) != 'null'),
    transaction_date TEXT NOT NULL,
    posting_date TEXT NOT NULL,
    amount_pence INTEGER NOT NULL CHECK (amount_pence > 0),
    currency TEXT NOT NULL CHECK (currency = 'GBP'),
    merchant TEXT NOT NULL,
    card_used TEXT NOT NULL,
    UNIQUE (source_account_id, reference),
    UNIQUE (id, source_account_id)
) STRICT;

CREATE TABLE source_observations (
    id INTEGER PRIMARY KEY,
    import_id INTEGER NOT NULL,
    source_account_id INTEGER NOT NULL,
    record_number INTEGER NOT NULL CHECK (record_number >= 2),
    source_text TEXT NOT NULL,
    disposition TEXT NOT NULL CHECK (disposition IN (
        'purchase', 'pending', 'credit', 'payment', 'fee',
        'malformed', 'unsupported', 'missing_reference', 'duplicate_reference', 'conflict'
    )),
    reason TEXT,
    purchase_id INTEGER,
    UNIQUE (import_id, record_number),
    FOREIGN KEY (import_id, source_account_id) REFERENCES imports(id, source_account_id),
    FOREIGN KEY (purchase_id, source_account_id) REFERENCES purchases(id, source_account_id),
    CHECK ((disposition = 'purchase' AND purchase_id IS NOT NULL)
        OR disposition != 'purchase'),
    CHECK (disposition = 'purchase' OR length(trim(coalesce(reason, ''))) > 0),
    CHECK (purchase_id IS NULL OR disposition IN ('purchase', 'conflict', 'duplicate_reference'))
) STRICT;

CREATE TABLE review_decisions (
    id INTEGER PRIMARY KEY,
    purchase_id INTEGER NOT NULL REFERENCES purchases(id),
    version INTEGER NOT NULL CHECK (version > 0),
    account TEXT CHECK (account IN ('personal', 'joint')),
    decided_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (purchase_id, version)
) STRICT;

CREATE TRIGGER review_decision_sequence BEFORE INSERT ON review_decisions
WHEN NEW.version != coalesce((SELECT max(version) FROM review_decisions WHERE purchase_id = NEW.purchase_id), 0) + 1
BEGIN SELECT RAISE(ABORT, 'nonsequential review version'); END;

CREATE VIEW current_account_decisions AS
SELECT d.* FROM review_decisions d
WHERE NOT EXISTS (
    SELECT 1 FROM review_decisions newer
    WHERE newer.purchase_id = d.purchase_id AND newer.version > d.version
);

CREATE INDEX observations_purchase ON source_observations(purchase_id);

CREATE TRIGGER imports_no_update BEFORE UPDATE ON imports
BEGIN SELECT RAISE(ABORT, 'imports are immutable'); END;
CREATE TRIGGER imports_no_delete BEFORE DELETE ON imports
BEGIN SELECT RAISE(ABORT, 'imports are immutable'); END;
CREATE TRIGGER purchases_no_update BEFORE UPDATE ON purchases
BEGIN SELECT RAISE(ABORT, 'purchases are immutable'); END;
CREATE TRIGGER purchases_no_delete BEFORE DELETE ON purchases
BEGIN SELECT RAISE(ABORT, 'purchases are immutable'); END;
CREATE TRIGGER observations_no_update BEFORE UPDATE ON source_observations
BEGIN SELECT RAISE(ABORT, 'observations are immutable'); END;
CREATE TRIGGER observations_no_delete BEFORE DELETE ON source_observations
BEGIN SELECT RAISE(ABORT, 'observations are immutable'); END;
CREATE TRIGGER decisions_no_update BEFORE UPDATE ON review_decisions
BEGIN SELECT RAISE(ABORT, 'decisions are append-only'); END;
CREATE TRIGGER decisions_no_delete BEFORE DELETE ON review_decisions
BEGIN SELECT RAISE(ABORT, 'decisions are append-only'); END;
