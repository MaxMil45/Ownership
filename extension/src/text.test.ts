import { describe, expect, it } from "vitest";
import {
  appPath,
  asTicker,
  checkBaseUrl,
  checkToken,
  cleanQuery,
  lastLine,
  officialTradeLine,
  safeUrl,
  whoLine,
  type Candidate,
} from "./text.js";

const base: Candidate = {
  key: "cik:1",
  display_name: "Jane Doe",
  kinds: ["insider"],
  filings: 3,
  first_on: "2020-01-01",
  last_on: "2026-01-01",
  exact: true,
  roles: [],
  trades: [],
  holdings: [],
  contests: [],
  official: null,
};

const role = (o: Partial<Candidate["roles"][number]>): Candidate["roles"][number] => ({
  issuer_cik: 1,
  issuer_name: "Acme",
  ticker: null,
  director: false,
  officer: false,
  ten_percent_owner: false,
  other: false,
  titles: [],
  last_on: "2026-01-01",
  ...o,
});

describe("cleanQuery", () => {
  it("makes selected text a search", () => {
    expect(cleanQuery("  “Elon\n Musk,”  ")).toBe("Elon Musk");
    expect(cleanQuery("Mr. Jensen Huang")).toBe("Jensen Huang");
    expect(cleanQuery("Pelosi’s")).toBe("Pelosi");
    expect(cleanQuery("(NVDA)")).toBe("NVDA");
    expect(cleanQuery(" x ")).toBeNull();
    expect(cleanQuery("ab ".repeat(300))!.length).toBeLessThanOrEqual(200);
  });
  it("sees tickers", () => {
    expect(asTicker("$tsla")).toBe("TSLA");
    expect(asTicker("Tim Cook")).toBeNull();
  });
});

describe("the token stays on this computer", () => {
  it("accepts only 127.0.0.1 and localhost over http", () => {
    expect(checkBaseUrl("http://127.0.0.1:8730")).toEqual({ ok: "http://127.0.0.1:8730" });
    expect(checkBaseUrl("http://localhost:9000/")).toEqual({ ok: "http://localhost:9000" });
    for (const bad of [
      "https://127.0.0.1:8730",
      "http://example.com:8730",
      "http://127.0.0.1.example.com",
      "http://user:pw@127.0.0.1:8730",
      "http://127.0.0.1:8730/v1",
      "nonsense",
    ]) {
      expect("error" in checkBaseUrl(bad), bad).toBe(true);
    }
  });
  it("checks the token's shape", () => {
    expect(checkToken(` ${"a".repeat(64)}\n`)).toBe("a".repeat(64));
    expect(checkToken("abc")).toBeNull();
  });
  it("links only to the SEC and the House Clerk", () => {
    expect(safeUrl("https://www.sec.gov/Archives/x")).not.toBeNull();
    expect(safeUrl("javascript:alert(1)")).toBeNull();
    expect(safeUrl("http://www.sec.gov/x")).toBeNull();
  });
});

describe("plain words", () => {
  it("says who and what last", () => {
    const c: Candidate = {
      ...base,
      roles: [
        role({ issuer_name: "Old", director: true, last_on: "2019-01-01" }),
        role({ director: true, officer: true, titles: ["CEO"] }),
      ],
      trades: [
        { transaction_date: "2026-01-02", code: "P", shares: 10, value: 25000, issuer_name: "Acme", ticker: "ACME", receipt: { url: "" } },
      ],
    };
    expect(whoLine(c)).toBe("CEO, director at Acme +1 more");
    expect(lastLine(c)).toBe("Bought ACME ($25.0K) on 2026-01-02");
    expect(lastLine(base)).toBeNull();
  });
  it("builds the app's address after the login link", () => {
    expect(appPath("http://127.0.0.1:8730/login?code=ab", { q: "Elon Musk" })).toBe(
      "http://127.0.0.1:8730/login?code=ab#/search?q=Elon+Musk",
    );
    expect(appPath("L", { name: "Musk", key: "cik:1" })).toBe("L#/lookup?name=Musk&key=cik%3A1");
    expect(appPath("L", { cik: 5 })).toBe("L#/company?cik=5");
  });
});

describe("members' trades", () => {
  it("are one short line each", () => {
    expect(
      officialTradeLine({
        owner: "SP",
        asset: "NVIDIA Corporation - Common Stock",
        ticker: "NVDA",
        tx_type: "P",
        tx_date: "2026-01-14",
        amount: "$1,000,001 - $5,000,000",
        amount_min: 1000001,
        amount_max: 5000000,
        report_url: "",
      }),
    ).toBe("Spouse bought NVDA, $1.00M–$5.00M, 2026-01-14");
  });
});
