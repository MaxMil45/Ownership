-- The events the rules find, for the local API (`oe serve`, GET /v1/events).
-- NOT part of the ledger: derived from the downloaded filings and the ledger
-- by `oe events-build`, which empties and refills it in one transaction, so
-- it may be dropped at any time. One row per alert key (ADR 0006).
CREATE TABLE event_feed (
    alert_key               TEXT PRIMARY KEY,
    rule_id                 TEXT NOT NULL,
    -- US Eastern date the event became knowable
    known_on                DATE,
    issuer_cik              BIGINT,
    issuer_name             TEXT,
    ticker                  TEXT,
    headline                TEXT NOT NULL,
    amount_usd              DOUBLE PRECISION,
    percent                 DOUBLE PRECISION,
    -- the alert as the webhook JSON (oe_alerts::render::json), served as is
    body                    TEXT NOT NULL,
    built_at                TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX event_feed_known ON event_feed (known_on DESC);
CREATE INDEX event_feed_rule ON event_feed (rule_id, known_on DESC);
CREATE INDEX event_feed_issuer ON event_feed (issuer_cik, known_on DESC);
CREATE INDEX event_feed_ticker ON event_feed (upper(ticker), known_on DESC);
