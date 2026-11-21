-- Replaces the index of 0003_bulk, which did not serve amendment linking.
-- Linking (oe-store UNLINKED_AMENDMENTS) looks for reports of one form type
-- accepted on one US Eastern day:
--     o.form_type = ... AND (o.accepted_at AT TIME ZONE 'America/New_York')::date = ...
-- An index on (form_type, accepted_at) cannot serve that date expression, so
-- PostgreSQL scanned every report of the form (353 ms per lookup on 2.4
-- million filings). This index holds the expression itself.
-- (timezone(text, timestamptz) is immutable, so it may be indexed.)
DROP INDEX IF EXISTS filing_form_accepted;

CREATE INDEX filing_form_eastern_day
    ON filing (form_type, ((accepted_at AT TIME ZONE 'America/New_York')::date));
