-- Point-in-time ledger schema (PostgreSQL 15+).
-- Same rules as crates/oe-ledger: append-only, two dates per fact,
-- amendments point at what they supersede and never overwrite it.

CREATE TABLE filing (
    accession       TEXT PRIMARY KEY,           -- 0000950170-26-012345
    form_type       TEXT NOT NULL,              -- 4, 4/A, 144, SC 13D, ...
    filer_cik       BIGINT NOT NULL,
    accepted_at     TIMESTAMPTZ NOT NULL,       -- EDGAR acceptance time = knowable time
    source_url      TEXT NOT NULL,
    fetched_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    raw_sha256      TEXT NOT NULL               -- detects silent changes to the source
);

CREATE TABLE insider_transaction (
    id                  BIGSERIAL PRIMARY KEY,
    accession           TEXT NOT NULL REFERENCES filing (accession),
    row_in_filing       INTEGER NOT NULL,
    issuer_cik          BIGINT NOT NULL,
    owner_cik           BIGINT NOT NULL,
    is_derivative       BOOLEAN NOT NULL,
    transaction_date    DATE NOT NULL,          -- valid time
    known_at            TIMESTAMPTZ NOT NULL,   -- knowable time (copied from filing.accepted_at)
    code                TEXT NOT NULL,
    shares              NUMERIC,
    price_per_share     NUMERIC,
    acquired_disposed   CHAR(1) CHECK (acquired_disposed IN ('A', 'D')),
    shares_after        NUMERIC,
    direct_indirect     CHAR(1) CHECK (direct_indirect IN ('D', 'I')),
    plan_10b5_1         BOOLEAN,
    supersedes_id       BIGINT REFERENCES insider_transaction (id),
    UNIQUE (accession, row_in_filing)
);

CREATE INDEX insider_transaction_issuer_date ON insider_transaction (issuer_cik, transaction_date);
CREATE INDEX insider_transaction_known_at ON insider_transaction (known_at);

-- Append-only: rows are never updated or deleted.
CREATE FUNCTION reject_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'ledger tables are append-only (% on %)', TG_OP, TG_TABLE_NAME;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER filing_append_only
    BEFORE UPDATE OR DELETE ON filing
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();

CREATE TRIGGER insider_transaction_append_only
    BEFORE UPDATE OR DELETE ON insider_transaction
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();

-- What the public could know at a given moment. Use it as:
--   SELECT * FROM insider_transaction_as_of('2026-03-31T23:59:59Z');
CREATE FUNCTION insider_transaction_as_of(as_of TIMESTAMPTZ)
RETURNS SETOF insider_transaction AS $$
    SELECT t.*
    FROM insider_transaction t
    WHERE t.known_at <= as_of
      AND NOT EXISTS (
          SELECT 1 FROM insider_transaction newer
          WHERE newer.supersedes_id = t.id
            AND newer.known_at <= as_of
      );
$$ LANGUAGE sql STABLE;

CREATE TABLE event (
    id              BIGSERIAL PRIMARY KEY,
    rule_id         TEXT NOT NULL,              -- insider.cluster_buy@1
    issuer_cik      BIGINT NOT NULL,
    window_start    DATE NOT NULL,
    window_end      DATE NOT NULL,
    known_at        TIMESTAMPTZ NOT NULL,       -- when the event became detectable
    detected_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    evidence        JSONB NOT NULL,             -- contributing accession numbers and rows
    UNIQUE (rule_id, issuer_cik, window_start)
);
