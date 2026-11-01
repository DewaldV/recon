-- Canonical overlap checks must also retain the original purchase currency.
-- Version 1 was preparation-only; pre-existing rows have unknown original currency.
ALTER TABLE purchases ADD COLUMN original_currency TEXT NOT NULL DEFAULT 'UNKNOWN';
