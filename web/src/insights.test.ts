import { describe, expect, it } from "vitest";
import { explainRule } from "./explain";
import {
  buyingClusters,
  monthName,
  summarizeCompany,
  summarizePerson,
  yearList,
  type Mark,
} from "./insights";

// Invented trades shaped like real patterns (no real filings in tests).
function mark(day: string, code: "P" | "S", value: number, owner = 1, plan?: boolean): Mark {
  return {
    day,
    issuer_cik: 9_999_901,
    issuer_name: "Example Motors Inc",
    ticker: "EXM",
    owner_cik: owner,
    owner_name: `Insider ${owner}`,
    code,
    trades: 1,
    shares: value / 100,
    value,
    priced_shares: value / 100,
    plan_trades: plan ? 1 : 0,
    plan_known: plan === undefined ? 0 : 1,
    receipt: { accession: "0000000000-00-000000", url: "https://www.sec.gov/x/", known_on: day, filing_date_only: true },
  };
}

describe("helpers", () => {
  it("writes year runs and month names", () => {
    expect(yearList([2015, 2017, 2018, 2019, 2020, 2025])).toBe("2015, 2017-2020 and 2025");
    expect(yearList([2021, 2022])).toBe("2021-2022");
    expect(yearList([2016])).toBe("2016");
    expect(monthName("2025-09-12")).toBe("September 2025");
  });

  it("counts cluster buys: three insiders within ten days", () => {
    const buys = [mark("2020-03-02", "P", 1, 1), mark("2020-03-05", "P", 1, 2), mark("2020-03-09", "P", 1, 3)];
    expect(buyingClusters(buys)).toBe(1);
    expect(buyingClusters(buys.slice(0, 2))).toBe(0);
    const spread = [mark("2020-03-02", "P", 1, 1), mark("2020-03-05", "P", 1, 2), mark("2020-03-20", "P", 1, 3)];
    expect(buyingClusters(spread)).toBe(0);
    const sameInsider = [mark("2020-03-02", "P", 1, 1), mark("2020-03-03", "P", 1, 1), mark("2020-03-04", "P", 1, 1)];
    expect(buyingClusters(sameInsider)).toBe(0);
  });
});

describe("a person's pattern", () => {
  // Small buys most years, two years of very large sales, then a big buy.
  const marks = [
    mark("2015-08-19", "P", 20e6),
    mark("2017-03-01", "P", 25e6),
    mark("2018-06-01", "P", 45e6),
    mark("2019-05-01", "P", 25e6),
    mark("2020-02-14", "P", 10e6),
    mark("2021-11-09", "S", 16.4e9),
    mark("2022-04-26", "S", 22.9e9),
    mark("2025-09-12", "P", 1e9),
  ];
  const [only] = summarizePerson(marks, "2026-11-25");
  const text = only!.lines.map((l) => l.text).join(" | ");

  it("says buyer or seller, with the years", () => {
    expect(only!.label).toBe("Example Motors Inc (EXM)");
    expect(text).toContain("Mostly a seller: bought $1.13B in 2015, 2017-2020 and 2025; sold $39.30B in 2021-2022.");
  });

  it("points out a buy after a long gap, and the largest", () => {
    expect(text).toContain("Latest buy: $1.00B in September 2025, the first since 2020 (5 years), the largest buy on record.");
  });

  it("names the heaviest selling year", () => {
    expect(text).toContain("Heaviest selling: 2022, $22.90B (1 sale).");
  });

  it("says when someone stopped trading", () => {
    const old = summarizePerson([mark("2019-05-01", "S", 1e6)], "2026-11-25")[0]!;
    expect(old.lines.map((l) => l.text)).toContain("No open-market trades here since May 2019.");
    expect(old.lines[0]!.text).toBe("Only sold: sold $1.00M in 2019.");
  });
});

describe("a company's pattern", () => {
  it("notices that nobody buys, who sells most, and plan sales", () => {
    const marks = [
      // The 10b5-1 box exists on filings from April 2023: earlier ones have no answer.
      ...[2019, 2020, 2021, 2022, 2023, 2024, 2025].map((y) =>
        mark(`${y}-05-01`, "S", y === 2021 ? 4e9 : 1e9, 1, y >= 2023 ? false : undefined),
      ),
      mark("2024-06-01", "S", 0.5e9, 2, true),
      ...Array.from({ length: 6 }, (_, i) => mark(`2026-0${i + 1}-15`, "S", 0.1e9, 1, true)),
    ];
    const lines = summarizeCompany(marks, "2026-11-25").map((l) => l.text);
    expect(lines[0]).toBe("Since 2019, 2 insiders sold $11.10B (14 sales), and none bought on the open market.");
    expect(lines).toContain("2021 was the heaviest year of selling: $4.00B, 36% of all sales.");
    expect(lines).toContain("95% of the selling was by Insider 1.");
    expect(lines.some((l) => l.startsWith("Selling in the last 12 months ($600.00M) is well below the yearly average of 2020-2025 ($1.58B)"))).toBe(true);
    // 7 of the 10 sales with the box answered were under a plan.
    expect(lines).toContain("70% of sales since April 2023 were made under pre-arranged 10b5-1 plans.");
  });

  it("reports the latest buy and how long ago it was", () => {
    const lines = summarizeCompany(
      [mark("2018-01-10", "P", 2e6, 1), mark("2018-01-12", "P", 1e6, 2), mark("2018-01-15", "P", 3e6, 3), mark("2025-01-01", "S", 1e6, 1)],
      "2026-11-25",
    ).map((l) => l.text);
    expect(lines).toContain("Most recent buy: Insider 3, January 2018, $3.00M; no insider has bought since (8 years).");
    expect(lines).toContain("1 time, three or more insiders bought within ten days of each other (a cluster buy).");
  });

  it("says so when there is nothing", () => {
    expect(summarizeCompany([], "2026-11-25")[0]!.text).toMatch(/No open-market/);
  });
});

describe("explanations", () => {
  it("has one for every rule family, whatever the version", () => {
    for (const r of [
      "insider.cluster_buy@1",
      "insider.planned_sale@2",
      "insider.plan_terminated@2",
      "activist.new_13d@1",
      "activist.passive_to_active@2",
      "activist.proxy_contest@2",
    ]) {
      expect(explainRule(r)?.text.length).toBeGreaterThan(50);
    }
    expect(explainRule("unknown.rule@1")).toBeUndefined();
  });
});

describe("trade chart data", () => {
  it("averages each day's price by shares and keeps the last years", async () => {
    const { dailyPrices, lastYears } = await import("./TradeChart");
    const a = { ...mark("2024-01-02", "S", 1000, 1), priced_shares: 10 }; // $100
    const b = { ...mark("2024-01-02", "S", 3000, 2), priced_shares: 10 }; // $300
    const c = { ...mark("2026-05-01", "P", 500, 1), priced_shares: 0 }; // no price
    expect(dailyPrices([a, b, c])).toEqual([{ day: "2024-01-02", price: 200 }]);
    expect(lastYears([a, c], 1).map((m) => m.day)).toEqual(["2026-05-01"]);
    expect(lastYears([a, c], null)).toHaveLength(2);
  });
});

describe("insider picker", () => {
  it("totals each insider's buys and sales, biggest first, latest name", async () => {
    const { insiderTotals } = await import("./InsiderPicker");
    const rows = insiderTotals([
      mark("2020-01-02", "S", 5e6, 1),
      { ...mark("2021-01-02", "P", 1e6, 1), owner_name: "Insider One (new name)" },
      mark("2022-03-04", "S", 9e6, 2),
      mark("2019-05-06", "P", 2e5, 3),
    ]);
    expect(rows.map((r) => [r.owner_cik, r.bought, r.sold, r.trades])).toEqual([
      [2, 0, 9e6, 1],
      [1, 1e6, 5e6, 2],
      [3, 2e5, 0, 1],
    ]);
    expect(rows[1]!.name).toBe("Insider One (new name)");
    expect(rows[1]!.last).toBe("2021-01-02");
  });
});

describe("track records", () => {
  it("says how the stock did a year after the person's buys", async () => {
    const { trackRecord } = await import("./insights");
    const o = (issuer: number, r12: number | null, x12: number | null, kind = "buy") => ({
      kind,
      issuer_cik: issuer,
      r: [0, 0, 0, r12],
      x: [0, 0, 0, x12],
    });
    const rows = [o(1, 0.3, 0.2), o(1, -0.1, -0.2), o(1, 0.5, 0.4), o(1, null, null), o(2, 0.9, 0.9), o(1, 0.8, 0.8, "sale")];
    expect(trackRecord(rows, 1)!.text).toBe(
      "A year after 3 buys became public, the stock was up after 2 (median +30%, +20% against the market). 1 later buy too recent to measure.",
    );
    expect(trackRecord([o(2, -0.25, -0.3)], 2)!.text).toBe(
      "A year after their buy became public, the stock was -25%, -30% against the market.",
    );
    expect(trackRecord(rows, 3)).toBeNull();
    expect(trackRecord([o(1, null, null)], 1)).toBeNull();
  });

  it("writes signed percentages", async () => {
    const { signedPct } = await import("./pages/History");
    expect(signedPct(0.1234)).toBe("+12.3%");
    expect(signedPct(-0.05, 0)).toBe("-5%");
    expect(signedPct(null)).toBe("–");
    expect(signedPct(Number.NaN)).toBe("–");
  });
});

describe("members of Congress", () => {
  it("summarizes reported trades in ranges", async () => {
    const { summarizeOfficial } = await import("./insights");
    const t = (day: string, type: string, ticker: string | null, min: number, max: number | null, owner: string | null = "SP") => ({
      owner,
      ticker,
      asset: ticker ? `Example ${ticker}` : "Example Bond",
      tx_type: type,
      tx_date: day,
      amount_min: min,
      amount_max: max,
    });
    const lines = summarizeOfficial([
      t("2024-01-05", "P", "EXA", 1001, 15000),
      t("2024-02-05", "P", "EXA", 1_000_001, 5_000_000),
      t("2024-03-05", "S", "EXB", 15001, 50000),
      t("2025-07-24", "P", "EXA", 50_000_001, null, null),
    ]).map((l) => l.text);
    expect(lines[0]).toBe("4 reported trades since January 2024: 3 buys and 1 sale, 75% of them in their spouse's name.");
    expect(lines[1]).toBe("Most traded: EXA (3), EXB (1).");
    expect(lines[2]).toBe("Reported in ranges, not exact amounts: between $51.02M and $55.07M in all.");
    expect(lines[3]).toBe("2 trades of over $1 million.");
    expect(lines[4]).toBe("Latest: bought EXA (over $50.00M) in July 2025.");
    expect(summarizeOfficial([])[0]!.text).toMatch(/No transactions/);
  });

  it("labels owners, sides and ranges", async () => {
    const { ownerLabel, rangeLabel, sideLabel } = await import("./pages/Congress");
    expect([ownerLabel(null), ownerLabel("SP"), ownerLabel("DC")]).toEqual(["Self", "Spouse", "Child"]);
    expect([sideLabel("P"), sideLabel("S"), sideLabel("S (partial)"), sideLabel("E")]).toEqual(["Buy", "Sale", "Partial sale", "Exchange"]);
    expect(rangeLabel(1001, 15000, "")).toBe("$1,001–$15.0K");
    expect(rangeLabel(1_000_001, null, "")).toBe("over $1.00M");
    expect(rangeLabel(null, null, "$?")).toBe("$?");
  });
});

describe("percent signs", () => {
  it("never writes -0.0%", async () => {
    const { signedPct } = await import("./pages/History");
    expect(signedPct(-0.0001)).toBe("0.0%");
    expect(signedPct(0.0001, 0)).toBe("0%");
    expect(signedPct(0.25, 0)).toBe("+25%");
  });
});
