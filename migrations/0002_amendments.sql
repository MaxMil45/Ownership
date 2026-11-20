-- Amendment linking and joint-filing co-owners (ADR 0005).
-- Every new table is append-only, like the ledger itself.

-- Every reporting owner of an ownership filing, in document order.
-- position 0 is filing.filer_cik; a joint filing has more than one row.
CREATE TABLE filing_owner (
    accession   TEXT NOT NULL REFERENCES filing (accession),
    position    INTEGER NOT NULL CHECK (position >= 0),
    owner_cik   BIGINT NOT NULL,
    PRIMARY KEY (accession, position)
);

CREATE INDEX filing_owner_owner ON filing_owner (owner_cik);

-- What an ownership document (Form 3/4/5 or amendment) says about itself.
CREATE TABLE ownership_filing (
    accession                   TEXT PRIMARY KEY REFERENCES filing (accession),
    issuer_cik                  BIGINT NOT NULL,
    -- Amendments only: the filing date of the report being amended. The XML
    -- gives no accession number, so this date is the link evidence.
    date_of_original_submission DATE
);

CREATE INDEX ownership_filing_issuer ON ownership_filing (issuer_cik);

-- The transaction rows of an amendment exactly as filed. They enter
-- insider_transaction only once the amendment is linked and restates the
-- rows it replaces; until then they are kept here and do not show in
-- insider_transaction_as_of.
CREATE TABLE amendment_transaction (
    accession           TEXT NOT NULL REFERENCES filing (accession),
    row_in_filing       INTEGER NOT NULL,
    issuer_cik          BIGINT NOT NULL,
    owner_cik           BIGINT NOT NULL,
    is_derivative       BOOLEAN NOT NULL,
    transaction_date    DATE NOT NULL,
    code                TEXT NOT NULL,
    shares              NUMERIC,
    price_per_share     NUMERIC,
    acquired_disposed   CHAR(1) CHECK (acquired_disposed IN ('A', 'D')),
    shares_after        NUMERIC,
    direct_indirect     CHAR(1) CHECK (direct_indirect IN ('D', 'I')),
    plan_10b5_1         BOOLEAN,
    PRIMARY KEY (accession, row_in_filing)
);

-- One row per amendment once its original is identified (or found to be
-- ambiguous). An amendment with no candidate original yet has no row here and
-- is tried again on the next load.
CREATE TABLE amendment_link (
    amendment_accession TEXT PRIMARY KEY REFERENCES filing (accession),
    -- NULL only when the outcome is 'ambiguous'.
    original_accession  TEXT REFERENCES filing (accession),
    -- The filing whose rows this amendment's rows supersede: the original, or
    -- an earlier amendment of it that was itself loaded.
    replaces_accession  TEXT REFERENCES filing (accession),
    outcome             TEXT NOT NULL CHECK (outcome IN ('loaded', 'held_back', 'ambiguous')),
    reason              TEXT NOT NULL,
    linked_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((outcome = 'ambiguous') = (original_accession IS NULL)),
    CHECK ((outcome = 'loaded') = (replaces_accession IS NOT NULL))
);

CREATE INDEX amendment_link_original ON amendment_link (original_accession);

CREATE TRIGGER filing_owner_append_only
    BEFORE UPDATE OR DELETE ON filing_owner
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();

CREATE TRIGGER ownership_filing_append_only
    BEFORE UPDATE OR DELETE ON ownership_filing
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();

CREATE TRIGGER amendment_transaction_append_only
    BEFORE UPDATE OR DELETE ON amendment_transaction
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();

CREATE TRIGGER amendment_link_append_only
    BEFORE UPDATE OR DELETE ON amendment_link
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
