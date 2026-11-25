-- Members of Congress and the House periodic transaction reports (PTRs) they
-- filed (ADR 0010). Members come from the public-domain congress-legislators
-- data set (Bioguide id, names, terms; committees as dated snapshots); reports
-- from the House Clerk's yearly index. The reports' PDFs stay in
-- data/officials and are not read yet. Use of these reports is limited by
-- law (5 U.S.C. 13107): local only, never published.
--
-- Not stored: birthdays, gender, office addresses and phone numbers, and the
-- FEC and OpenSecrets ids (no donor data).

CREATE TABLE official (
    bioguide_id     TEXT PRIMARY KEY,
    first_name      TEXT NOT NULL,
    last_name       TEXT NOT NULL,
    middle_name     TEXT,
    suffix          TEXT,
    nickname        TEXT,
    official_full   TEXT
);

-- One row per term in the House (`rep`) or the Senate (`sen`).
CREATE TABLE official_term (
    bioguide_id     TEXT NOT NULL REFERENCES official (bioguide_id),
    chamber         TEXT NOT NULL CHECK (chamber IN ('rep', 'sen')),
    start_on        DATE NOT NULL,
    end_on          DATE NOT NULL,
    state           TEXT NOT NULL,
    district        INTEGER,            -- House only; 0 = at large
    party           TEXT,
    PRIMARY KEY (bioguide_id, chamber, start_on)
);

CREATE INDEX official_term_seat ON official_term (state, district);

CREATE TABLE committee (
    committee_id    TEXT PRIMARY KEY,   -- HSAG, or HSAG15 for a subcommittee
    parent_id       TEXT,
    chamber         TEXT NOT NULL,      -- house, senate, joint
    name            TEXT NOT NULL
);

-- Who sat on which committee, as the data set said on `taken_on` (the day
-- `oe` loaded it). The data set holds only current membership, so committee
-- history builds up from the first load on.
CREATE TABLE committee_membership (
    taken_on        DATE NOT NULL,
    committee_id    TEXT NOT NULL,
    bioguide_id     TEXT NOT NULL,
    rank            INTEGER,
    title           TEXT,               -- Chairman, Ranking Member, ...
    side            TEXT,               -- majority, minority
    PRIMARY KEY (taken_on, committee_id, bioguide_id)
);

-- A PTR as the House Clerk's index lists it.
CREATE TABLE house_report (
    doc_id          TEXT PRIMARY KEY,
    year            INTEGER NOT NULL,
    filing_type     TEXT NOT NULL,
    prefix          TEXT NOT NULL,
    last_name       TEXT NOT NULL,
    first_name      TEXT NOT NULL,
    suffix          TEXT NOT NULL,
    state_district  TEXT NOT NULL,
    filed_on        DATE,               -- as the index states it
    -- Paper filing, scanned (a seven-digit document id; measured 2013-2026:
    -- every such PDF has no text, every eight-digit one has text).
    scanned         BOOLEAN NOT NULL,
    source_url      TEXT NOT NULL,
    -- When `oe` first saw the PDF; for history downloaded later, the filing
    -- date is the only "known on", and may be earlier than public posting.
    first_seen_at   TIMESTAMPTZ,
    pdf_sha256      TEXT
);

CREATE INDEX house_report_filed ON house_report (filed_on);

-- Which member filed it: a Bioguide id with how it was matched, or held
-- back with the reason (never guessed).
CREATE TABLE house_report_link (
    doc_id          TEXT PRIMARY KEY REFERENCES house_report (doc_id),
    bioguide_id     TEXT REFERENCES official (bioguide_id),
    basis           TEXT,
    held_back       TEXT,
    linked_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((bioguide_id IS NULL) <> (held_back IS NULL))
);

CREATE INDEX house_report_link_member ON house_report_link (bioguide_id);

CREATE TRIGGER official_append_only BEFORE UPDATE OR DELETE ON official
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER official_term_append_only BEFORE UPDATE OR DELETE ON official_term
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER committee_append_only BEFORE UPDATE OR DELETE ON committee
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER committee_membership_append_only BEFORE UPDATE OR DELETE ON committee_membership
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER house_report_append_only BEFORE UPDATE OR DELETE ON house_report
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
CREATE TRIGGER house_report_link_append_only BEFORE UPDATE OR DELETE ON house_report_link
    FOR EACH ROW EXECUTE FUNCTION reject_mutation();
