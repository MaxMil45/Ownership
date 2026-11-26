import { describe, expect, it } from "vitest";
import type { Candidate } from "./api";
import { inShort, joinWords, oneLine, roleWords } from "./describe";
import { GLOSSARY, TERM } from "./glossary";
import { asTicker } from "./pages/Search";

const receipt = { accession: "0001", url: "https://www.sec.gov/x", known_on: "2026-01-05", filing_date_only: false };
const role = (o: Partial<Candidate["roles"][number]>): Candidate["roles"][number] => ({
  issuer_cik: 1,
  issuer_name: "Acme Corp",
  ticker: "ACME",
  director: false,
  officer: false,
  ten_percent_owner: false,
  other: false,
  titles: [],
  first_on: "2020-01-01",
  last_on: "2026-01-01",
  filings: 3,
  ...o,
});
const base: Candidate = {
  key: "cik:9",
  display_name: "Jane Doe",
  kinds: ["insider"],
  other_names: [],
  filings: 12,
  first_on: "2019-03-01",
  last_on: "2026-01-05",
  exact: true,
  roles: [],
  trades: [],
  holdings: [],
  contests: [],
  official: null,
};

describe("describe", () => {
  it("words a role", () => {
    expect(roleWords(role({ officer: true, titles: ["CEO"], director: true }))).toBe("CEO and director");
    expect(roleWords(role({ ten_percent_owner: true }))).toBe("10% owner");
    expect(roleWords(role({}))).toBe("insider");
    expect(joinWords(["a", "b", "c"])).toBe("a, b and c");
  });

  it("gives one line, the latest role first", () => {
    const c = {
      ...base,
      roles: [role({ director: true, last_on: "2021-01-01", issuer_name: "Old Co" }), role({ officer: true, titles: ["CFO"] })],
    };
    expect(oneLine(c)).toBe("CFO at Acme Corp and 1 other company");
  });

  it("writes the in-short lines from the filings only", () => {
    const c: Candidate = {
      ...base,
      roles: [role({ officer: true, titles: ["CEO"] })],
      trades: [
        { transaction_date: "2026-01-02", code: "S", shares: 1000, price: 10, value: 10000, issuer_name: "Acme Corp", ticker: "ACME", receipt },
      ],
    };
    const lines = inShort(c);
    expect(lines[0]).toBe("Jane Doe appears in 12 filings from 2019 to 2026.");
    expect(lines[1]).toBe("Reported as CEO at Acme Corp (ACME).");
    expect(lines[2]).toBe("Latest open-market trade: sold 1,000 shares of ACME for about $10.0K on 2026-01-02.");
    expect(inShort({ ...c, trades: [], exact: false }).at(-1)).toMatch(/partial match/);
  });

  it("says when an insider has no open-market trades", () => {
    expect(inShort({ ...base, roles: [role({ director: true })] })[2]).toMatch(/No open-market/);
  });
});

describe("search", () => {
  it("recognises tickers", () => {
    expect(asTicker("nvda")).toBe("NVDA");
    expect(asTicker("$TSLA")).toBe("TSLA");
    expect(asTicker("BRK.B")).toBe("BRK.B");
    expect(asTicker("Elon Musk")).toBeNull();
  });
});

describe("glossary", () => {
  it("has unique ids and a short note for each word", () => {
    expect(new Set(GLOSSARY.map((g) => g.id)).size).toBe(GLOSSARY.length);
    for (const g of GLOSSARY) expect(g.short.length).toBeGreaterThan(10);
    expect(TERM.form4?.term).toBe("Form 4");
  });
});

describe("members of Congress", () => {
  it("are described by their term and reports, not a filing count", () => {
    const c: Candidate = {
      ...base,
      kinds: ["official"],
      official: {
        bioguide: "X000001",
        name: "Jane Doe",
        terms: ["rep CA-11 2025-01-03..2027-01-03 Democrat"],
        committees: [],
        reports: 5,
        scanned_reports: 0,
        first_filed: "2020-01-01",
        last_filed: "2026-11-06",
        recent_reports: [],
        use_limits: "",
      },
    };
    expect(inShort(c)).toEqual([
      "Member of Congress (latest term: rep CA-11 2025-01-03..2027-01-03 Democrat), with 5 trade reports on file, the latest filed 2026-11-06.",
    ]);
  });
});
