-- Indexes for the lookups the API makes (measured with docker/bench.sql).

-- A 13D/G holder without a CIK is found by the names it filed under
-- (oe_store::lookup): 41 ms as a scan of every reporting person.
CREATE INDEX schedule13_person_name_no_cik ON schedule13_person (name) WHERE cik IS NULL;

-- Word search over events (GET /v1/events?q=): ILIKE '%word%' can use a
-- trigram index instead of reading the whole feed (128 ms on 53,042 events).
-- The extension lives in public so every schema (the tests make their own)
-- sees the same operator class.
CREATE EXTENSION IF NOT EXISTS pg_trgm SCHEMA public;
CREATE INDEX event_feed_headline_trgm ON event_feed USING gin (headline public.gin_trgm_ops);
CREATE INDEX event_feed_issuer_trgm ON event_feed USING gin (issuer_name public.gin_trgm_ops);
