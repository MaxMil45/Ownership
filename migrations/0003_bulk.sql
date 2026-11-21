-- Filings loaded from the SEC's quarterly Insider Transactions Data Sets
-- (ADR 0008) next to filings parsed from their own XML.

-- 'xml': parsed from the filing; accepted_at is the exact acceptance time.
-- 'bulk': from a quarterly data set; it gives only the filing date, so
--         accepted_at is 23:59:59 US Eastern on that date. That is never
--         earlier than the real acceptance, so "as known on" queries cannot
--         see a bulk filing too early. Numbers there have two decimals.
-- Existing rows are XML (the default fills them in; no UPDATE runs).
ALTER TABLE filing
    ADD COLUMN source TEXT NOT NULL DEFAULT 'xml' CHECK (source IN ('xml', 'bulk'));

-- Amendment linking looks up reports by owner; bulk loads are large.
CREATE INDEX filing_form_accepted ON filing (form_type, accepted_at);
