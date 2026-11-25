-- Names and roles as filed, and the name index the lookup reads (ADR 0010, 2).

-- Every reporting owner of a Form 3/4/5 as the filing names them: name,
-- director / officer / 10% owner / other flags and officer title. Same key as
-- filing_owner. Filled on load, and for filings loaded before this migration
-- by loading the same source again (rows only for a filing whose stored
-- fingerprint matches, as for filing_owner). Never an address.
CREATE TABLE filing_owner_role (
    accession               TEXT NOT NULL REFERENCES filing (accession),
    position                INTEGER NOT NULL CHECK (position >= 0),
    owner_cik               BIGINT NOT NULL,
    owner_name              TEXT NOT NULL,
    is_director             BOOLEAN NOT NULL,
    is_officer              BOOLEAN NOT NULL,
    is_ten_percent_owner    BOOLEAN NOT NULL,
    is_other                BOOLEAN NOT NULL,
    officer_title           TEXT,
    PRIMARY KEY (accession, position)
);

CREATE INDEX filing_owner_role_owner ON filing_owner_role (owner_cik);

-- The issuer of a Form 3/4/5 as the filing names it.
CREATE TABLE ownership_issuer (
    accession               TEXT PRIMARY KEY REFERENCES filing (accession),
    issuer_cik              BIGINT NOT NULL,
    issuer_name             TEXT NOT NULL,
    ticker                  TEXT
);

CREATE INDEX ownership_issuer_issuer ON ownership_issuer (issuer_cik);

CREATE TRIGGER filing_owner_role_append_only BEFORE UPDATE OR DELETE ON filing_owner_role
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER ownership_issuer_append_only BEFORE UPDATE OR DELETE ON ownership_issuer
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();

-- "Is this row superseded?" (insider_transaction_as_of and the lookup) without
-- reading every row: 45,943 of 4.9 million rows supersede another (2026-11-24).
CREATE INDEX insider_transaction_supersedes ON insider_transaction (supersedes_id)
    WHERE supersedes_id IS NOT NULL;

-- The name index. NOT part of the ledger: derived from it by `oe people-index`,
-- which empties and rebuilds both tables in one transaction, so they may be
-- dropped at any time. One candidate per identifier (cik:<CIK>,
-- bioguide:<id>, holder:<name tokens> for a 13D/G entity with no CIK);
-- candidates are never merged (ADR 0010, 2).
CREATE TABLE people_candidate (
    candidate_key           TEXT PRIMARY KEY,
    display_name            TEXT NOT NULL,
    -- insider, holder, contest, official (comma-separated, sorted)
    kinds                   TEXT NOT NULL,
    -- every token of every name variant, space-separated
    tokens                  TEXT NOT NULL,
    -- the tokens of display_name, space-separated (exact-match ranking)
    display_tokens          TEXT NOT NULL,
    filings                 BIGINT NOT NULL,
    first_on                DATE,
    last_on                 DATE,
    built_at                TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE people_token (
    token                   TEXT NOT NULL,
    candidate_key           TEXT NOT NULL REFERENCES people_candidate (candidate_key) ON DELETE CASCADE,
    PRIMARY KEY (token, candidate_key)
);

-- Every name a candidate was filed under.
CREATE TABLE people_name (
    candidate_key           TEXT NOT NULL REFERENCES people_candidate (candidate_key) ON DELETE CASCADE,
    name                    TEXT NOT NULL,
    PRIMARY KEY (candidate_key, name)
);
