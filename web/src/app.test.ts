import { describe, expect, it } from "vitest";
import { eventsQuery } from "./api";
import { niceMax, monthlyEvents, monthlyFlow } from "./charts";
import { daysBefore, money, percent, ruleFamily, ruleLabel, safeUrl } from "./format";
import { hashFor, parseHash } from "./router";

describe("format", () => {
  it("writes money as the Rust side does", () => {
    expect(money(1234)).toBe("$1,234");
    expect(money(56_789)).toBe("$56.8K");
    expect(money(1_250_000)).toBe("$1.25M");
    expect(money(3_100_000_000)).toBe("$3.10B");
    expect(money(-5)).toBe("-$5");
  });

  it("rounds percentages for reading", () => {
    expect(percent(5.612004818614034)).toBe("5.61%");
    expect(percent(64.8)).toBe("64.8%");
    expect(percent(0.0001849601793117026)).toBe("0.000%");
    expect(percent(0.1209)).toBe("0.12%");
  });

  it("links only to the SEC and the House Clerk over https", () => {
    expect(safeUrl("https://www.sec.gov/Archives/edgar/data/1/2/")).not.toBeNull();
    expect(
      safeUrl("https://disclosures-clerk.house.gov/public_disc/ptr-pdfs/2026/20035780.pdf"),
    ).not.toBeNull();
    expect(safeUrl("http://www.sec.gov/x")).toBeNull();
    expect(safeUrl("https://www.sec.gov.evil.example/x")).toBeNull();
    expect(safeUrl("javascript:alert(1)")).toBeNull();
    expect(safeUrl("not a url")).toBeNull();
  });

  it("names rules by family, whatever the version", () => {
    expect(ruleFamily("insider.cluster_buy@1")).toBe("insider.cluster_buy");
    expect(ruleLabel("insider.planned_sale@2")).toBe("Planned sale (Form 144)");
    expect(ruleLabel("something.new@1")).toBe("something.new");
  });

  it("counts days back across month ends", () => {
    expect(daysBefore("2026-03-01", 1)).toBe("2026-02-28");
    expect(daysBefore("2026-11-25", 30)).toBe("2026-10-26");
  });
});

describe("api", () => {
  it("keeps only the filters that are set", () => {
    expect(eventsQuery({})).toBe("");
    expect(eventsQuery({ ticker: " TSLA ", q: "", limit: 50, offset: 0 })).toBe(
      "?ticker=TSLA&limit=50&offset=0",
    );
    expect(eventsQuery({ q: "a b&c" })).toBe("?q=a+b%26c");
  });
});

describe("router", () => {
  it("round-trips a path and its parameters", () => {
    expect(parseHash("#/events?ticker=TSLA&page=2")).toEqual({
      path: "/events",
      params: { ticker: "TSLA", page: "2" },
    });
    expect(parseHash("")).toEqual({ path: "/", params: {} });
    expect(hashFor("/events", { ticker: "TSLA", q: undefined, page: "" })).toBe("#/events?ticker=TSLA");
  });
});

describe("charts", () => {
  it("rounds an axis up to a round number", () => {
    expect(niceMax(0)).toBe(1);
    expect(niceMax(0.9)).toBe(1);
    expect(niceMax(7)).toBe(8);
    expect(niceMax(130)).toBe(150);
    expect(niceMax(262)).toBe(300);
    expect(niceMax(2_300_000)).toBe(2_500_000);
  });

  it("sums days into months", () => {
    const m = monthlyFlow({
      implausible_rows_left_out: 0,
      days: [
        { day: "2026-10-31", buy_value: 1, buys: 1, sell_value: 2, sells: 1 },
        { day: "2026-11-01", buy_value: 10, buys: 1, sell_value: 20, sells: 1 },
        { day: "2026-11-02", buy_value: 5, buys: 1, sell_value: 0, sells: 0 },
      ],
    });
    expect(m).toEqual([
      { month: "2026-10", buy: 1, sell: 2 },
      { month: "2026-11", buy: 15, sell: 20 },
    ]);
  });

  it("groups event counts by month and rule family", () => {
    const m = monthlyEvents({
      counts: [
        { day: "2026-11-01", rule_id: "insider.planned_sale@2", events: 3 },
        { day: "2026-11-02", rule_id: "insider.planned_sale@2", events: 4 },
        { day: "2026-11-02", rule_id: "activist.new_13d@1", events: 1 },
      ],
    });
    expect(m).toHaveLength(1);
    expect(m[0]?.row.get("insider.planned_sale")).toBe(7);
    expect(m[0]?.row.get("activist.new_13d")).toBe(1);
  });
});

describe("chart ranges", () => {
  it("keeps the last years of months, or everything", async () => {
    const { inRange } = await import("./charts");
    const rows = ["2020-12", "2021-11", "2021-12", "2025-12", "2026-11"].map((month) => ({ month }));
    expect(inRange(rows, null)).toHaveLength(5);
    expect(inRange(rows, 1).map((r) => r.month)).toEqual(["2025-12", "2026-11"]);
    expect(inRange(rows, 5).map((r) => r.month)).toEqual(["2021-12", "2025-12", "2026-11"]);
    expect(inRange([], 1)).toEqual([]);
  });
});
