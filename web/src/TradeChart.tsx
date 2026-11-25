import { useMemo, useRef, useState } from "react";
import { count, money, safeUrl } from "./format";
import { monthName, type Mark } from "./insights";
import { niceMax } from "./charts";

const W = 900;
const H = 300;
const PAD = { l: 56, r: 12, t: 18, b: 24 };
const DAY = 86_400_000;
const t = (day: string) => Date.parse(`${day}T00:00:00Z`) / DAY;
/** A gap longer than this breaks the price line: no insider traded. */
const GAP_DAYS = 45;
const LABELS = 4;
/** Horizontal room a label needs, in chart units. */
const MIN_LABEL_GAP = 150;

export interface PricePoint {
  day: string;
  price: number;
}

/** The average price insiders traded at each day (amounts as filed). */
export function dailyPrices(marks: Mark[]): PricePoint[] {
  const by = new Map<string, { v: number; s: number }>();
  for (const m of marks) {
    if (m.priced_shares <= 0) continue;
    const e = by.get(m.day) ?? { v: 0, s: 0 };
    e.v += m.value;
    e.s += m.priced_shares;
    by.set(m.day, e);
  }
  return [...by.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([day, e]) => ({ day, price: e.v / e.s }));
}

/** Marks from the last `years` years of the data (all when null). */
export function lastYears<T extends { day: string }>(rows: T[], years: number | null): T[] {
  if (years === null || rows.length === 0) return rows;
  const last = rows.reduce((a, r) => (r.day > a ? r.day : a), rows[0]!.day);
  const from = `${Number(last.slice(0, 4)) - years}${last.slice(4)}`;
  return rows.filter((r) => r.day > from);
}

const surname = (name: string) => name.split(/[\s,]+/)[0] ?? name;

interface Tip {
  m: Mark;
  x: number;
  y: number;
}

export function TradeChart({
  marks,
  years,
  selected = null,
  others = "hide",
  market = null,
  officials = [],
}: {
  marks: Mark[];
  years: number | null;
  /** Daily market closes (split-adjusted). With them the line is the market
   * price and each marker sits on it; without, the line is insiders' prices. */
  market?: [string, number][] | null;
  /** Members of Congress's trades of this stock: drawn as diamonds on the line
   * (reports give ranges, not prices, so all the same size). */
  officials?: { day: string; buy: boolean; label: string; url: string }[];
  /** Whose trades to mark; `null` is everyone. The price line always uses everyone's. */
  selected?: ReadonlySet<number> | null;
  /** What to do with everyone else's trades. */
  others?: "hide" | "fade";
}) {
  const box = useRef<HTMLDivElement>(null);
  const [tip, setTip] = useState<Tip | null>(null);
  const [offTip, setOffTip] = useState<{ label: string; x: number; y: number } | null>(null);
  const shown = useMemo(() => lastYears(marks, years), [marks, years]);
  const insiderPrices = useMemo(() => dailyPrices(shown), [shown]);
  const marketPrices = useMemo(() => {
    if (!market || market.length === 0 || shown.length === 0) return null;
    const from = shown[0]!.day;
    const to = shown[shown.length - 1]!.day;
    const pts = market.filter(([d]) => d >= from && d <= to).map(([day, price]) => ({ day, price }));
    return pts.length > 1 ? pts : null;
  }, [market, shown]);
  const prices = marketPrices ?? insiderPrices;
  // The line's price on (or just before) a day: where a diamond sits.
  const priceOn = (day: string): number | null => {
    if (prices.length === 0 || day < prices[0]!.day) return null;
    let lo = 0;
    let hi = prices.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (prices[mid]!.day <= day) lo = mid;
      else hi = mid - 1;
    }
    return prices[lo]!.price;
  };
  // The market close on (or just before) each day, for placing markers.
  const closeOn = useMemo(() => {
    if (!marketPrices) return null;
    return (day: string): number | null => {
      let lo = 0;
      let hi = marketPrices.length - 1;
      // A trade dated a day or two before the first close in view (a weekend)
      // sits on that first close, never on its unadjusted reported price.
      if (day < marketPrices[0]!.day) return marketPrices[0]!.price;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        if (marketPrices[mid]!.day <= day) lo = mid;
        else hi = mid - 1;
      }
      return marketPrices[lo]!.price;
    };
  }, [marketPrices]);
  const priced = shown.filter((m) => m.priced_shares > 0);
  const isOn = (m: Mark) => selected === null || selected.has(m.owner_cik);
  if (priced.length === 0) {
    return <div className="empty">No open-market trades with a price in this period.</div>;
  }

  const x0 = t(shown[0]!.day);
  const x1 = Math.max(x0 + 30, t(shown[shown.length - 1]!.day));
  const reportedAvg = (m: Mark) => m.value / m.priced_shares;
  // Where a marker sits: on the market line when there is one.
  const avg = (m: Mark) => (closeOn ? (closeOn(m.day) ?? reportedAvg(m)) : reportedAvg(m));
  const ys = [...priced.map(avg), ...(marketPrices ?? []).map((p) => p.price)];
  const yMax = niceMax(Math.max(...ys) * 1.05);
  const iw = W - PAD.l - PAD.r;
  const ih = H - PAD.t - PAD.b;
  const X = (day: string) => PAD.l + ((t(day) - x0) / (x1 - x0)) * iw;
  const Y = (p: number) => PAD.t + ih - (p / yMax) * ih;
  // Marker sizes compare the trades on show (one person's are not dwarfed).
  const sized = priced.filter(isOn);
  const maxValue = Math.max(1, ...(sized.length ? sized : priced).map((m) => m.value));
  const size = (m: Mark) => 3.5 + 8.5 * Math.sqrt(Math.min(1, m.value / maxValue));

  // The price line, broken where nobody traded for a while.
  const segments: PricePoint[][] = [];
  for (const p of prices) {
    const seg = segments[segments.length - 1];
    if (seg && t(p.day) - t(seg[seg.length - 1]!.day) <= GAP_DAYS) seg.push(p);
    else segments.push([p]);
  }
  const path = (seg: PricePoint[]) => seg.map((p, i) => `${i ? "L" : "M"}${X(p.day).toFixed(1)},${Y(p.price).toFixed(1)}`).join("");

  // Year (or month) ticks along the bottom.
  const spanDays = x1 - x0;
  const ticks: string[] = [];
  const y0 = Number(shown[0]!.day.slice(0, 4));
  const yEnd = Number(shown[shown.length - 1]!.day.slice(0, 4));
  if (spanDays > 3 * 365) {
    const step = spanDays > 9 * 365 ? 2 : 1;
    for (let y = y0 + 1; y <= yEnd; y += step) ticks.push(`${y}-01-01`);
  } else {
    for (let y = y0; y <= yEnd; y++)
      for (let mo = 1; mo <= 12; mo += 3) {
        const d = `${y}-${String(mo).padStart(2, "0")}-01`;
        if (t(d) >= x0 && t(d) <= x1) ticks.push(d);
      }
  }

  const drawn = priced.filter((m) => others === "fade" || isOn(m));
  // Draw the faded ones first, small ones before big ones (big stay visible).
  const order = [...drawn].sort((a, b) => Number(isOn(a)) - Number(isOn(b)) || a.value - b.value);
  // The biggest trades get a label, unless one is already placed nearby
  // (big trades bunch together: a label each would overlap).
  const labelled: Mark[] = [];
  for (const m of [...priced].filter(isOn).sort((a, b) => b.value - a.value)) {
    if (labelled.length >= LABELS) break;
    const near = labelled.some(
      (l) => Math.abs(X(l.day) - X(m.day)) < MIN_LABEL_GAP && Math.abs(Y(avg(l)) - Y(avg(m))) < 40,
    );
    if (!near) labelled.push(m);
  }

  const show = (m: Mark, el: Element) => {
    const host = box.current?.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    if (host) setTip({ m, x: r.left + r.width / 2 - host.left, y: r.top - host.top });
  };

  return (
    <div className="chart trade-chart" ref={box} style={{ position: "relative" }}>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Insider trade prices with each buy and sale marked">
        {[0, 0.25, 0.5, 0.75, 1].map((f) => (
          <g key={f}>
            <line x1={PAD.l} x2={W - PAD.r} y1={Y(yMax * f)} y2={Y(yMax * f)} stroke="var(--grid)" />
            <text x={PAD.l - 6} y={Y(yMax * f) + 4} textAnchor="end">
              ${(yMax * f).toLocaleString("en-US", { maximumFractionDigits: yMax < 10 ? 2 : 0 })}
            </text>
          </g>
        ))}
        {ticks.map((d) => (
          <text key={d} x={X(d)} y={H - 6} textAnchor="middle">
            {spanDays > 3 * 365 ? d.slice(0, 4) : d.slice(0, 7)}
          </text>
        ))}
        {segments.map((seg) =>
          seg.length > 1 ? (
            <path key={seg[0]!.day} d={path(seg)} fill="none" stroke="var(--text-muted)" strokeWidth={1.2} />
          ) : (
            <circle key={seg[0]!.day} cx={X(seg[0]!.day)} cy={Y(seg[0]!.price)} r={1.2} fill="var(--text-muted)" />
          ),
        )}
        {order.map((m) => {
          const cx = X(m.day);
          const cy = Y(avg(m));
          const s = size(m);
          const buy = m.code === "P";
          const d = buy
            ? `M${cx},${cy - s} L${cx + s},${cy + s * 0.8} L${cx - s},${cy + s * 0.8} Z`
            : `M${cx},${cy + s} L${cx + s},${cy - s * 0.8} L${cx - s},${cy - s * 0.8} Z`;
          const href = safeUrl(m.receipt.url) ?? undefined;
          return (
            <a
              key={`${m.day}-${m.owner_cik}-${m.code}`}
              href={href}
              target="_blank"
              rel="noopener noreferrer"
              aria-label={`${m.owner_name} ${buy ? "bought" : "sold"} ${money(m.value)} on ${m.day}`}
              onMouseEnter={(e) => show(m, e.currentTarget)}
              onFocus={(e) => show(m, e.currentTarget)}
              onMouseLeave={() => setTip(null)}
              onBlur={() => setTip(null)}
            >
              <path
                d={d}
                fill={buy ? "var(--buy)" : "var(--sell)"}
                fillOpacity={isOn(m) ? 0.85 : 0.18}
                stroke="var(--surface-1)"
                strokeWidth={0.8}
              />
            </a>
          );
        })}
        {officials
          .filter((o) => o.day >= shown[0]!.day && o.day <= shown[shown.length - 1]!.day)
          .map((o, i) => {
            const p = priceOn(o.day);
            if (p === null) return null;
            const cx = X(o.day);
            const cy = Y(p);
            const r = 6;
            const href = safeUrl(o.url) ?? undefined;
            return (
              <a
                key={`off-${o.day}-${i}`}
                href={href}
                target="_blank"
                rel="noopener noreferrer"
                aria-label={o.label}
                onMouseEnter={(e) => {
                  const host = box.current?.getBoundingClientRect();
                  const rr = e.currentTarget.getBoundingClientRect();
                  if (host) setOffTip({ label: o.label, x: rr.left + rr.width / 2 - host.left, y: rr.top - host.top });
                }}
                onMouseLeave={() => setOffTip(null)}
              >
                <path
                  d={`M${cx},${cy - r} L${cx + r},${cy} L${cx},${cy + r} L${cx - r},${cy} Z`}
                  fill={o.buy ? "var(--s5)" : "var(--surface-1)"}
                  stroke="var(--s5)"
                  strokeWidth={1.6}
                />
              </a>
            );
          })}
        {labelled.map((m) => {
          const cx = X(m.day);
          const cy = Y(avg(m));
          const right = cx < W * 0.7;
          const dy = m.code === "P" ? -size(m) - 6 : size(m) + 14;
          return (
            <text
              key={`label-${m.day}-${m.owner_cik}-${m.code}`}
              x={cx + (right ? 6 : -6)}
              y={cy + dy}
              textAnchor={right ? "start" : "end"}
              className="mark-label"
            >
              {surname(m.owner_name)} {m.code === "P" ? "bought" : "sold"} {money(m.value)}
            </text>
          );
        })}
      </svg>
      {offTip && (
        <div className="tip" style={{ left: offTip.x, top: offTip.y }} role="status">
          {offTip.label}
          <div className="muted">click to open the report</div>
        </div>
      )}
      {tip && (
        <div className="tip" style={{ left: tip.x, top: tip.y }} role="status">
          <strong>{tip.m.owner_name}</strong>
          <div>
            {tip.m.code === "P" ? "Bought" : "Sold"} {count(Math.round(tip.m.shares))} shares of{" "}
            {tip.m.ticker ?? tip.m.issuer_name}
          </div>
          <div>
            {money(tip.m.value)} at about ${(tip.m.value / tip.m.priced_shares).toFixed(2)} a share (as reported)
          </div>
          <div className="muted">
            {tip.m.day} · {tip.m.trades} trade{tip.m.trades === 1 ? "" : "s"}
            {tip.m.plan_trades > 0 ? ` · ${tip.m.plan_trades} under a 10b5-1 plan` : ""}
          </div>
          <div className="muted">Filed {tip.m.receipt.known_on} · click to open the filing</div>
        </div>
      )}
      {selected !== null && !priced.some(isOn) && (
        <div className="muted">None of the chosen insiders traded with a price in this period.</div>
      )}
      <div className="legend">
        <span>
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path d="M6,1 L11,10 L1,10 Z" fill="var(--buy)" />
          </svg>
          Buy
        </span>
        <span>
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path d="M6,11 L11,2 L1,2 Z" fill="var(--sell)" />
          </svg>
          Sale
        </span>
        <span>
          <i className="sw" style={{ background: "var(--text-muted)", height: 2 }} />{" "}
          {marketPrices ? "Market close (Stooq, adjusted for splits)" : "Price insiders traded at"}
        </span>
        {officials.length > 0 && (
          <span>
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <path d="M6,1 L11,6 L6,11 L1,6 Z" fill="var(--s5)" />
            </svg>
            Congress member (filled = buy)
          </span>
        )}
        <span className="muted">Bigger marker = more money. {monthName(shown[0]!.day)} to {monthName(shown[shown.length - 1]!.day)}.</span>
      </div>
      <details className="table-view">
        <summary>Largest trades in this period</summary>
        <table>
          <thead>
            <tr>
              <th>Date</th>
              <th>Who</th>
              <th>Trade</th>
              <th className="num">Value</th>
              <th className="num">Avg. price</th>
            </tr>
          </thead>
          <tbody>
            {[...priced]
              .filter(isOn)
              .sort((a, b) => b.value - a.value)
              .slice(0, 15)
              .map((m) => (
                <tr key={`row-${m.day}-${m.owner_cik}-${m.code}`}>
                  <td>{m.day}</td>
                  <td>{m.owner_name}</td>
                  <td>{m.code === "P" ? "buy" : "sale"}</td>
                  <td className="num">{money(m.value)}</td>
                  <td className="num">${(m.value / m.priced_shares).toFixed(2)}</td>
                </tr>
              ))}
          </tbody>
        </table>
      </details>
    </div>
  );
}
