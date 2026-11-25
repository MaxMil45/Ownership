-- Daily prices from a file the user downloaded (Stooq's ASCII database,
-- adjusted by Stooq for splits and dividends), and what happened after each
-- event and insider trade. None of this is part of the ledger: every table is
-- emptied and refilled by `oe prices-import` and `oe outcomes-build`. Price
-- data stays on this computer: never committed or published (ADR 0003).

CREATE TABLE price_daily (
    symbol      TEXT NOT NULL,              -- Stooq name: aapl.us, brk-b.us
    day         DATE NOT NULL,
    close       DOUBLE PRECISION NOT NULL CHECK (close > 0),
    -- The trading-day number within the symbol's series (1 = its first day):
    -- "21 trading days later" is n + 21.
    n           INTEGER NOT NULL,
    PRIMARY KEY (symbol, day)
);
CREATE UNIQUE INDEX price_daily_n ON price_daily (symbol, n);

CREATE TABLE price_import (
    imported_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    source      TEXT NOT NULL,              -- the file or folder given
    symbols     INTEGER NOT NULL,
    days        BIGINT NOT NULL,
    first_day   DATE,
    last_day    DATE
);

-- Which price series is which company's: the issuer's latest Form 3/4/5
-- ticker, accepted only when its closes agree with the prices insiders
-- reported on the same days (a reused or wrong ticker would not).
CREATE TABLE issuer_price_series (
    issuer_cik      BIGINT PRIMARY KEY,
    symbol          TEXT NOT NULL,
    compared        INTEGER NOT NULL,       -- insider trades compared
    median_gap      DOUBLE PRECISION,       -- median |close / reported price - 1|
    status          TEXT NOT NULL CHECK (status IN ('ok', 'mismatch', 'unchecked'))
);

-- Returns after each event and each insider trade day, from the first close
-- after the day the filing became public (never the trade's own date: the
-- public could not act before), against SPY over the same days.
CREATE TABLE outcome (
    kind        TEXT NOT NULL CHECK (kind IN ('event', 'buy', 'sale')),
    ref         TEXT NOT NULL,              -- alert key, or issuer|owner|day|code
    issuer_cik  BIGINT NOT NULL,
    owner_cik   BIGINT,                     -- trades only
    family      TEXT NOT NULL,              -- rule family, or insider.buy / insider.sale
    traded_on   DATE,                       -- trades only
    known_on    DATE NOT NULL,
    value       DOUBLE PRECISION,           -- trades: dollars as filed (plausible rows)
    entry_day   DATE NOT NULL,
    r21  DOUBLE PRECISION, r63  DOUBLE PRECISION, r126 DOUBLE PRECISION, r252 DOUBLE PRECISION,
    -- the same minus SPY's return over the same trading days
    x21  DOUBLE PRECISION, x63  DOUBLE PRECISION, x126 DOUBLE PRECISION, x252 DOUBLE PRECISION,
    PRIMARY KEY (kind, ref)
);
CREATE INDEX outcome_owner ON outcome (owner_cik, kind) WHERE owner_cik IS NOT NULL;
CREATE INDEX outcome_issuer ON outcome (issuer_cik, kind);
