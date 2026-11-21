-- Form 144, Schedule 13D/13G, contested-proxy filings and 10-Q/10-K insider
-- trading arrangements (Item 408) in the ledger (ADR 0009).
--
-- Every filing has its receipt in `filing` (exact acceptance time, raw
-- fingerprint, source URL); the tables below hold what it says. Known-at is
-- `filing.accepted_at`. Amendments (144/A, 13D/A, 13G/A) are stored as their
-- own filings: each is a complete statement as of its own acceptance, so "as
-- known on" a date means the latest statement accepted by then.
-- Not stored: street addresses (Form 144 and 13D/G can carry home
-- addresses) and CUSIPs (a licensed identifier; ADR 0003).

CREATE TABLE form144_notice (
    accession               TEXT PRIMARY KEY REFERENCES filing (accession),
    issuer_cik              BIGINT NOT NULL,
    issuer_name             TEXT NOT NULL,
    seller_name             TEXT NOT NULL,
    -- Every ticked relationship as written: Officer, Director, 10% Stockholder, ...
    relationships           TEXT[] NOT NULL,
    notice_date             DATE,
    plan_adoption_dates     DATE[] NOT NULL,
    nothing_sold_past_3_months BOOLEAN,
    -- Sum of the units sold in the past three months (all prior-sale blocks).
    sold_past_3_months      NUMERIC,
    remarks                 TEXT
);

CREATE INDEX form144_notice_issuer ON form144_notice (issuer_cik);

-- One row per `securitiesInformation` block of a notice.
CREATE TABLE form144_security (
    accession               TEXT NOT NULL REFERENCES form144_notice (accession),
    position                INTEGER NOT NULL,
    class_title             TEXT NOT NULL,
    units                   NUMERIC,
    aggregate_market_value  NUMERIC,
    units_outstanding       NUMERIC,
    approx_sale_date        DATE,
    exchange                TEXT,
    broker                  TEXT,
    PRIMARY KEY (accession, position)
);

CREATE TABLE schedule13_filing (
    accession               TEXT PRIMARY KEY REFERENCES filing (accession),
    submission_type         TEXT NOT NULL,      -- SCHEDULE 13D, SCHEDULE 13G/A, ...
    schedule                CHAR(1) NOT NULL CHECK (schedule IN ('D', 'G')),
    amendment_no            INTEGER,
    previous_accession      TEXT,               -- as the filer states it; often missing
    issuer_cik              BIGINT NOT NULL,
    issuer_name             TEXT NOT NULL,
    class_title             TEXT NOT NULL,
    event_date              DATE,
    previously_filed_13g    BOOLEAN,
    rules                   TEXT[] NOT NULL,    -- 13G: Rule 13d-1(b), (c), (d)
    purpose_of_transaction  TEXT,               -- 13D Item 4, as written
    signature_date          DATE
);

CREATE INDEX schedule13_filing_issuer ON schedule13_filing (issuer_cik);

-- One row per reporting person, in document order.
CREATE TABLE schedule13_person (
    accession               TEXT NOT NULL REFERENCES schedule13_filing (accession),
    position                INTEGER NOT NULL,
    name                    TEXT NOT NULL,
    cik                     BIGINT,             -- 13D only; a 13G does not say
    member_of_group         TEXT,
    citizenship_or_organization TEXT,
    sole_voting             NUMERIC,
    shared_voting           NUMERIC,
    sole_dispositive        NUMERIC,
    shared_dispositive      NUMERIC,
    aggregate_shares        NUMERIC,
    percent_of_class        NUMERIC,
    person_type             TEXT,
    fund_type               TEXT,
    PRIMARY KEY (accession, position)
);

CREATE INDEX schedule13_person_cik ON schedule13_person (cik) WHERE cik IS NOT NULL;

-- A contested-solicitation filing (PREC14A, DEFC14A, DFAN14A, ...) as its
-- SGML header describes it: the subject company and everyone who filed.
CREATE TABLE contest_filing (
    accession               TEXT PRIMARY KEY REFERENCES filing (accession),
    form                    TEXT NOT NULL,
    subject_cik             BIGINT NOT NULL,
    subject_name            TEXT NOT NULL
);

CREATE INDEX contest_filing_subject ON contest_filing (subject_cik);

CREATE TABLE contest_filer (
    accession               TEXT NOT NULL REFERENCES contest_filing (accession),
    position                INTEGER NOT NULL,
    cik                     BIGINT NOT NULL,
    name                    TEXT NOT NULL,
    PRIMARY KEY (accession, position)
);

-- One row per insider trading arrangement a 10-Q or 10-K tags (Item 408).
CREATE TABLE trading_arrangement (
    accession               TEXT NOT NULL REFERENCES filing (accession),
    position                INTEGER NOT NULL,
    registrant_cik          BIGINT NOT NULL,
    report_type             TEXT,               -- 10-Q, 10-K, ...
    period_end              DATE,
    arrangement             TEXT,               -- the plan's XBRL member, if tagged
    person                  TEXT,
    title                   TEXT,
    rule_10b5_1_adopted     BOOLEAN NOT NULL,
    rule_10b5_1_terminated  BOOLEAN NOT NULL,
    non_rule_adopted        BOOLEAN NOT NULL,
    non_rule_terminated     BOOLEAN NOT NULL,
    adoption_date           DATE,
    termination_date        DATE,
    expiration_date         DATE,
    securities_amount       NUMERIC,
    duration_days           INTEGER,
    terms_text              TEXT,
    PRIMARY KEY (accession, position)
);

CREATE INDEX trading_arrangement_registrant ON trading_arrangement (registrant_cik);

CREATE TRIGGER form144_notice_append_only BEFORE UPDATE OR DELETE ON form144_notice
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER form144_security_append_only BEFORE UPDATE OR DELETE ON form144_security
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER schedule13_filing_append_only BEFORE UPDATE OR DELETE ON schedule13_filing
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER schedule13_person_append_only BEFORE UPDATE OR DELETE ON schedule13_person
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER contest_filing_append_only BEFORE UPDATE OR DELETE ON contest_filing
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER contest_filer_append_only BEFORE UPDATE OR DELETE ON contest_filer
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER trading_arrangement_append_only BEFORE UPDATE OR DELETE ON trading_arrangement
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
