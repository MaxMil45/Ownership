-- Timings of the queries the API, the lookup and the builds use most.
-- Run: Get-Content docker\bench.sql | docker exec -i oe-postgres psql -U oe -d ownership
-- (read-only; prints one "Time:" line per query)
\timing on
\o /dev/null
\echo flow_by_day (GET /v1/flow)
SELECT (known_at AT TIME ZONE 'America/New_York')::date, count(*)
FROM insider_transaction_as_of(now()) WHERE NOT is_derivative AND code IN ('P','S') GROUP BY 1;
\echo status: transactions known now
SELECT count(*) FROM insider_transaction_as_of(now());
\echo status: joint filings
SELECT count(*) FROM filing_owner WHERE position = 1;
\echo lookup: candidates for two words
SELECT candidate_key FROM people_token WHERE token = ANY(ARRAY['ELON','MUSK'])
GROUP BY candidate_key HAVING count(*) = 2;
\echo lookup: insider roles (Musk)
SELECT i.issuer_cik, count(DISTINCT r.accession) FROM filing_owner_role r
JOIN ownership_issuer i USING (accession) JOIN filing f USING (accession)
WHERE r.owner_cik = 1494730 GROUP BY i.issuer_cik;
\echo lookup: insider latest trades (Musk)
SELECT t.id FROM filing_owner fo JOIN insider_transaction t ON t.accession = fo.accession
JOIN filing f ON f.accession = t.accession
WHERE fo.owner_cik = 1494730 AND NOT t.is_derivative AND t.code IN ('P','S')
  AND NOT EXISTS (SELECT 1 FROM insider_transaction n WHERE n.supersedes_id = t.id)
ORDER BY f.accepted_at DESC LIMIT 5;
\echo lookup: holder by name (13G, no CIK)
SELECT count(*) FROM schedule13_person WHERE cik IS NULL AND name = ANY(ARRAY['Elliott Investment Management L.P.']);
\echo events: ticker page
SELECT body FROM event_feed WHERE upper(ticker) = 'TSLA' ORDER BY known_on DESC NULLS LAST, amount_usd DESC NULLS LAST, alert_key LIMIT 50;
\echo events: words
SELECT count(*) FROM event_feed WHERE headline ILIKE '%elliott%' OR issuer_name ILIKE '%elliott%';
\echo events: newest page
SELECT body FROM event_feed ORDER BY known_on DESC NULLS LAST, amount_usd DESC NULLS LAST, alert_key LIMIT 50;
\echo events: counts per day and rule
SELECT known_on, rule_id, count(*) FROM event_feed WHERE known_on IS NOT NULL GROUP BY 1, 2;
\echo latest tickers (events-build)
SELECT DISTINCT ON (i.issuer_cik) i.issuer_cik, i.ticker FROM ownership_issuer i JOIN filing f USING (accession)
WHERE i.ticker IS NOT NULL ORDER BY i.issuer_cik, f.accepted_at DESC;
\echo ledger purchases (events-build)
SELECT count(*) FROM insider_transaction_as_of(now()) t
LEFT JOIN filing_owner_role r ON r.accession = t.accession AND r.position = 0
LEFT JOIN ownership_issuer i ON i.accession = t.accession
WHERE NOT t.is_derivative AND t.code = 'P';
\echo trade history for the routine filter (events-build)
SELECT count(*) FROM (SELECT DISTINCT t.issuer_cik, fo.owner_cik, t.transaction_date, t.code
FROM insider_transaction_as_of(now()) t JOIN filing_owner fo ON fo.accession = t.accession
WHERE t.transaction_date >= '2011-01-01' AND NOT t.is_derivative AND t.code IN ('P','S')) x;
\o
