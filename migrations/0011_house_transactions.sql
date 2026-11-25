-- The transactions in House members' periodic transaction reports, read
-- from the PDFs (ADR 0011). Derived from the reports in data/officials (never
-- committed or published; use limited by 5 U.S.C. 13107): `oe ptr-load`
-- empties and refills both tables, so a better reader replaces old readings.

CREATE TABLE house_transaction (
    doc_id          TEXT NOT NULL REFERENCES house_report (doc_id),
    line_no         INTEGER NOT NULL,           -- order in the report, from 1
    -- SP spouse, JT joint, DC dependent child; NULL the member. Never a name.
    owner           TEXT CHECK (owner IN ('SP', 'JT', 'DC')),
    asset           TEXT NOT NULL,              -- as written
    ticker          TEXT,                       -- in brackets after the asset, upper case
    asset_type      TEXT,                       -- ST stock, OP option, GS, MF, ...
    tx_type         TEXT NOT NULL CHECK (tx_type IN ('P', 'S', 'S (partial)', 'E')),
    tx_date         DATE,
    notified_on     DATE,
    amount          TEXT NOT NULL,              -- the range as written
    amount_min      BIGINT,
    amount_max      BIGINT,                     -- NULL for an open top band
    filing_status   TEXT,                       -- New / Amended ...
    description     TEXT,
    comments        TEXT,
    subholding_of   TEXT,                       -- the account it sits in
    -- The company, when the ticker is one company's latest Form 3/4/5 ticker.
    issuer_cik      BIGINT,
    PRIMARY KEY (doc_id, line_no)
);
CREATE INDEX house_transaction_issuer ON house_transaction (issuer_cik) WHERE issuer_cik IS NOT NULL;
CREATE INDEX house_transaction_ticker ON house_transaction (upper(ticker));
CREATE INDEX house_transaction_date ON house_transaction (tx_date);

-- Which reports were read, by which reader version, with what result.
CREATE TABLE house_report_read (
    doc_id          TEXT PRIMARY KEY REFERENCES house_report (doc_id),
    transactions    INTEGER NOT NULL,
    problems        TEXT,                       -- fields that could not be read
    reader          TEXT NOT NULL,
    read_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Members' trades measured like insiders' (migration 0010): what the stock
-- did after the report was filed.
ALTER TABLE outcome DROP CONSTRAINT outcome_kind_check;
ALTER TABLE outcome ADD CONSTRAINT outcome_kind_check
    CHECK (kind IN ('event', 'buy', 'sale', 'official_buy', 'official_sale'));
ALTER TABLE outcome ADD COLUMN bioguide_id TEXT;
CREATE INDEX outcome_bioguide ON outcome (bioguide_id) WHERE bioguide_id IS NOT NULL;
