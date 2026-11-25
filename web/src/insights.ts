// Plain-English patterns from insiders' open-market trades (GET /v1/trades).
//
// Every sentence states something the filings show: counts, dollar amounts,
// dates and names. None says why an insider traded; the filings do not say.
// What a kind of trade usually means is in explain.ts, kept apart.

import { money } from "./format";

export interface Mark {
  day: string;
  issuer_cik: number;
  issuer_name: string;
  ticker: string | null;
  owner_cik: number;
  owner_name: string;
  code: string;
  trades: number;
  shares: number;
  value: number;
  priced_shares: number;
  plan_trades: number;
  plan_known: number;
  receipt: { accession: string; url: string; known_on: string; filing_date_only: boolean };
}

export interface Insight {
  text: string;
  /** Colour hint for the dot in front of the sentence. */
  tone: "buy" | "sell" | "note";
}

const yearOf = (day: string) => Number(day.slice(0, 4));
const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);
const pct = (part: number, whole: number) => (whole > 0 ? Math.round((part / whole) * 100) : 0);
const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
export function monthName(day: string): string {
  return `${MONTHS[Number(day.slice(5, 7)) - 1] ?? "?"} ${day.slice(0, 4)}`;
}

/** `[2015, 2016, 2017, 2020]` -> `2015-2017 and 2020`. */
export function yearList(years: number[]): string {
  const ys = [...new Set(years)].sort((a, b) => a - b);
  const runs: string[] = [];
  for (let i = 0; i < ys.length; ) {
    let j = i;
    while (j + 1 < ys.length && ys[j + 1] === ys[j]! + 1) j++;
    runs.push(i === j ? String(ys[i]) : `${ys[i]}-${ys[j]}`);
    i = j + 1;
  }
  if (runs.length <= 1) return runs.join("");
  return `${runs.slice(0, -1).join(", ")} and ${runs[runs.length - 1]}`;
}

function byYear(marks: Mark[]): Map<number, number> {
  const m = new Map<number, number>();
  for (const x of marks) m.set(yearOf(x.day), (m.get(yearOf(x.day)) ?? 0) + x.value);
  return m;
}

/** Days with buys by at least `min` different insiders within `window` days. */
export function buyingClusters(buys: Mark[], min = 3, window = 10): number {
  const sorted = [...buys].sort((a, b) => a.day.localeCompare(b.day));
  const t = (d: string) => Date.parse(`${d}T00:00:00Z`) / 86_400_000;
  let clusters = 0;
  let i = 0;
  while (i < sorted.length) {
    const start = t(sorted[i]!.day);
    let j = i;
    while (j + 1 < sorted.length && t(sorted[j + 1]!.day) - start < window) j++;
    const who = new Set(sorted.slice(i, j + 1).map((m) => m.owner_cik));
    if (who.size >= min) {
      clusters++;
      i = j + 1;
    } else i++;
  }
  return clusters;
}

/** What the trades of every insider of one company show. */
export function summarizeCompany(marks: Mark[], today: string): Insight[] {
  const out: Insight[] = [];
  if (marks.length === 0) {
    return [{ text: "No open-market buys or sales by insiders are in the filings.", tone: "note" }];
  }
  const buys = marks.filter((m) => m.code === "P");
  const sales = marks.filter((m) => m.code === "S");
  const first = marks[0]!.day;
  const firstYear = yearOf(first);
  const buyValue = sum(buys.map((m) => m.value));
  const saleValue = sum(sales.map((m) => m.value));
  const who = (ms: Mark[]) => new Set(ms.map((m) => m.owner_cik)).size;

  out.push({
    text:
      `Since ${firstYear}, ${plural(who(sales), "insider")} sold ${money(saleValue)} ` +
      `(${plural(sum(sales.map((m) => m.trades)), "sale")})` +
      (buys.length
        ? ` and ${plural(who(buys), "insider")} bought ${money(buyValue)} (${plural(sum(buys.map((m) => m.trades)), "buy")}).`
        : ", and none bought on the open market."),
    tone: saleValue >= buyValue ? "sell" : "buy",
  });

  if (buys.length) {
    const last = buys[buys.length - 1]!;
    const yearsAgo = yearOf(today) - yearOf(last.day);
    out.push({
      text:
        `Most recent buy: ${last.owner_name}, ${monthName(last.day)}, ${money(last.value)}` +
        (yearsAgo >= 2 ? `; no insider has bought since (${plural(yearsAgo, "year")}).` : "."),
      tone: "buy",
    });
    const clusters = buyingClusters(buys);
    if (clusters > 0) {
      out.push({
        text: `${plural(clusters, "time")}, three or more insiders bought within ten days of each other (a cluster buy).`,
        tone: "buy",
      });
    }
  }

  if (sales.length) {
    const years = byYear(sales);
    const [topYear, topValue] = [...years.entries()].sort((a, b) => b[1] - a[1])[0]!;
    if (years.size > 1 && saleValue > 0) {
      out.push({
        text: `${topYear} was the heaviest year of selling: ${money(topValue)}, ${pct(topValue, saleValue)}% of all sales.`,
        tone: "sell",
      });
    }
    const byOwner = new Map<string, number>();
    for (const m of sales) byOwner.set(m.owner_name, (byOwner.get(m.owner_name) ?? 0) + m.value);
    const [topName, topOwner] = [...byOwner.entries()].sort((a, b) => b[1] - a[1])[0]!;
    if (byOwner.size > 1 && pct(topOwner, saleValue) >= 50) {
      out.push({ text: `${pct(topOwner, saleValue)}% of the selling was by ${topName}.`, tone: "sell" });
    }
    // The last twelve months against the average of earlier full years.
    const cutoff = `${yearOf(today) - 1}${today.slice(4)}`;
    const recent = sum(sales.filter((m) => m.day > cutoff).map((m) => m.value));
    const fullYears = [...years.keys()].filter((y) => y >= firstYear + 1 && y < yearOf(today));
    if (fullYears.length >= 3) {
      const avg = sum(fullYears.map((y) => years.get(y) ?? 0)) / fullYears.length;
      const ratio = avg > 0 ? recent / avg : 0;
      const word = ratio > 1.5 ? "well above" : ratio < 0.67 ? "well below" : "about the same as";
      out.push({
        text: `Selling in the last 12 months (${money(recent)}) is ${word} the yearly average of ${fullYears[0]}-${fullYears[fullYears.length - 1]} (${money(avg)}).`,
        tone: "note",
      });
    }
    const known = sales.filter((m) => m.plan_known > 0);
    const planned = sum(known.map((m) => m.plan_trades));
    const knownTrades = sum(known.map((m) => m.plan_known));
    if (knownTrades >= 5) {
      out.push({
        text: `${pct(planned, knownTrades)}% of sales since April 2023 were made under pre-arranged 10b5-1 plans.`,
        tone: "note",
      });
    }
  }
  return out;
}

/** One insider's pattern at one company. */
export interface PersonAtCompany {
  issuer_cik: number;
  label: string;
  lines: Insight[];
}

/** What one insider's trades show, company by company (most traded first). */
export function summarizePerson(marks: Mark[], today: string): PersonAtCompany[] {
  const by = new Map<number, Mark[]>();
  for (const m of marks) by.set(m.issuer_cik, [...(by.get(m.issuer_cik) ?? []), m]);
  const out: PersonAtCompany[] = [];
  for (const [cik, ms] of by) {
    ms.sort((a, b) => a.day.localeCompare(b.day));
    const latest = ms[ms.length - 1]!;
    const label = latest.ticker ? `${latest.issuer_name} (${latest.ticker})` : latest.issuer_name;
    const buys = ms.filter((m) => m.code === "P");
    const sales = ms.filter((m) => m.code === "S");
    const bv = sum(buys.map((m) => m.value));
    const sv = sum(sales.map((m) => m.value));
    const lines: Insight[] = [];
    const kind =
      buys.length === 0 ? "only sold" : sales.length === 0 ? "only bought" : sv > bv * 2 ? "mostly a seller" : bv > sv * 2 ? "mostly a buyer" : "both bought and sold";
    lines.push({
      text:
        `${kind[0]!.toUpperCase()}${kind.slice(1)}` +
        (buys.length ? `: bought ${money(bv)} in ${yearList(buys.map((m) => yearOf(m.day)))}` : "") +
        (sales.length ? `${buys.length ? ";" : ":"} sold ${money(sv)} in ${yearList(sales.map((m) => yearOf(m.day)))}` : "") +
        ".",
      tone: sv >= bv ? "sell" : "buy",
    });
    if (buys.length) {
      const last = buys[buys.length - 1]!;
      const before = buys.filter((m) => m.day < last.day.slice(0, 7));
      const prev = before[before.length - 1];
      const lastMonth = buys.filter((m) => m.day.slice(0, 7) === last.day.slice(0, 7));
      const lastValue = sum(lastMonth.map((m) => m.value));
      const gap = prev ? yearOf(last.day) - yearOf(prev.day) : 0;
      const biggestBefore = Math.max(0, ...before.map((m) => m.value));
      let text = `Latest buy: ${money(lastValue)} in ${monthName(last.day)}`;
      if (prev && gap >= 2) text += `, the first since ${yearOf(prev.day)} (${plural(gap, "year")})`;
      if (before.length && lastValue > biggestBefore) text += ", the largest buy on record";
      lines.push({ text: `${text}.`, tone: "buy" });
    }
    if (sales.length) {
      const years = byYear(sales);
      const [topYear, topValue] = [...years.entries()].sort((a, b) => b[1] - a[1])[0]!;
      if (years.size > 1) {
        lines.push({
          text: `Heaviest selling: ${topYear}, ${money(topValue)} (${plural(sum(sales.filter((m) => yearOf(m.day) === topYear).map((m) => m.trades)), "sale")}).`,
          tone: "sell",
        });
      }
    }
    const lastDay = latest.day;
    const idle = yearOf(today) - yearOf(lastDay);
    if (idle >= 2) {
      lines.push({ text: `No open-market trades here since ${monthName(lastDay)}.`, tone: "note" });
    }
    out.push({ issuer_cik: cik, label, lines });
  }
  const weight = (p: PersonAtCompany) => sum((by.get(p.issuer_cik) ?? []).map((m) => m.value));
  return out.sort((a, b) => weight(b) - weight(a));
}

/** What a person's buys at one company were followed by, a year on. */
export function trackRecord(
  outcomes: { kind: string; issuer_cik: number; r: (number | null)[]; x: (number | null)[] }[],
  issuer: number,
): Insight | null {
  const buys = outcomes.filter((o) => o.kind === "buy" && o.issuer_cik === issuer);
  const measured = buys.filter((o) => o.r[3] !== null && o.r[3] !== undefined);
  if (measured.length === 0) return null;
  const r = measured.map((o) => o.r[3] as number);
  const x = measured.map((o) => o.x[3]).filter((v): v is number => v !== null && v !== undefined);
  const up = r.filter((v) => v > 0).length;
  const med = (vs: number[]) => {
    const s = [...vs].sort((a, b) => a - b);
    const m = Math.floor(s.length / 2);
    return s.length % 2 ? s[m]! : (s[m - 1]! + s[m]!) / 2;
  };
  const signed = (v: number) => `${v >= 0 ? "+" : ""}${Math.round(v * 100)}%`;
  const vs = x.length ? `, ${signed(med(x))} against the market` : "";
  const text =
    measured.length === 1
      ? `A year after their buy became public, the stock was ${signed(r[0]!)}${vs}.`
      : `A year after ${plural(measured.length, "buy")} became public, the stock was up after ${up} ` +
        `(median ${signed(med(r))}${vs}).`;
  const pending = buys.length - measured.length;
  return {
    text: text + (pending > 0 ? ` ${plural(pending, "later buy")} too recent to measure.` : ""),
    tone: "note",
  };
}

/** What a member of Congress's reported trades show (ranges, not figures). */
export function summarizeOfficial(
  trades: { owner: string | null; ticker: string | null; asset: string; tx_type: string; tx_date: string | null; amount_min: number | null; amount_max: number | null }[],
): Insight[] {
  if (trades.length === 0) return [{ text: "No transactions read from their reports.", tone: "note" }];
  const dated = trades.filter((t) => t.tx_date).sort((a, b) => (a.tx_date! < b.tx_date! ? -1 : 1));
  const buys = trades.filter((t) => t.tx_type === "P");
  const sales = trades.filter((t) => t.tx_type.startsWith("S"));
  const out: Insight[] = [];
  const first = dated[0]?.tx_date;
  const byOwner = (code: string | null) => trades.filter((t) => t.owner === code).length;
  const owners: [string, number][] = [
    ["their own", byOwner(null)],
    ["their spouse's", byOwner("SP")],
    ["joint", byOwner("JT")],
    ["a dependent child's", byOwner("DC")],
  ];
  const [mainOwner, mainCount] = owners.sort((a, b) => b[1] - a[1])[0]!;
  out.push({
    text:
      `${plural(trades.length, "reported trade")}${first ? ` since ${monthName(first)}` : ""}: ` +
      `${plural(buys.length, "buy")} and ${plural(sales.length, "sale")}` +
      (mainCount / trades.length >= 0.6 ? `, ${pct(mainCount, trades.length)}% of them in ${mainOwner} name.` : "."),
    tone: buys.length >= sales.length ? "buy" : "sell",
  });
  const counts = new Map<string, number>();
  for (const t of trades) if (t.ticker) counts.set(t.ticker, (counts.get(t.ticker) ?? 0) + 1);
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 5);
  if (top.length) {
    out.push({ text: `Most traded: ${top.map(([k, n]) => `${k} (${n})`).join(", ")}.`, tone: "note" });
  }
  const lo = sum(trades.map((t) => t.amount_min ?? 0));
  const hi = sum(trades.map((t) => t.amount_max ?? t.amount_min ?? 0));
  out.push({
    text: `Reported in ranges, not exact amounts: between ${money(lo)} and ${money(hi)} in all.`,
    tone: "note",
  });
  const big = trades.filter((t) => (t.amount_min ?? 0) >= 1_000_001);
  if (big.length) {
    out.push({ text: `${plural(big.length, "trade")} of over $1 million.`, tone: big.every((t) => t.tx_type === "P") ? "buy" : "note" });
  }
  const last = dated[dated.length - 1];
  if (last) {
    const verb = last.tx_type === "P" ? "bought" : last.tx_type === "E" ? "exchanged" : "sold";
    const range = last.amount_max ? `${money(last.amount_min ?? 0)}-${money(last.amount_max)}` : `over ${money((last.amount_min ?? 1) - 1)}`;
    out.push({
      text: `Latest: ${verb} ${last.ticker ?? last.asset.slice(0, 60)} (${range}) in ${monthName(last.tx_date!)}.`,
      tone: last.tx_type === "P" ? "buy" : "sell",
    });
  }
  return out;
}
