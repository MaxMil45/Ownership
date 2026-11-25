import { useMemo } from "react";
import { count, money, monthOf } from "./format";
import type { CountsResponse, FlowResponse } from "./api";
import { RULES, ruleFamily } from "./format";

const W = 900;
const H = 240;
const PAD = { l: 52, r: 8, t: 8, b: 22 };

/** A round number at or above `v`, for the top of an axis. */
export function niceMax(v: number): number {
  if (!(v > 0)) return 1;
  const p = Math.pow(10, Math.floor(Math.log10(v)));
  for (const m of [1, 2, 2.5, 5, 10]) if (m * p >= v) return m * p;
  return 10 * p;
}

/** Buying and selling per month, summed from the daily flow. */
export function monthlyFlow(flow: FlowResponse) {
  const by = new Map<string, { buy: number; sell: number }>();
  for (const d of flow.days) {
    const m = monthOf(d.day);
    const e = by.get(m) ?? { buy: 0, sell: 0 };
    e.buy += d.buy_value;
    e.sell += d.sell_value;
    by.set(m, e);
  }
  return [...by.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([month, v]) => ({ month, ...v }));
}

/** Events per month by rule family, in the order of `RULES`. */
export function monthlyEvents(c: CountsResponse) {
  const by = new Map<string, Map<string, number>>();
  for (const r of c.counts) {
    const m = monthOf(r.day);
    const fam = ruleFamily(r.rule_id);
    const row = by.get(m) ?? new Map<string, number>();
    row.set(fam, (row.get(fam) ?? 0) + r.events);
    by.set(m, row);
  }
  return [...by.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([month, row]) => ({ month, row }));
}

/** The months to show for a range: everything, or the last `years` years. */
export function inRange<T extends { month: string }>(rows: T[], years: number | null): T[] {
  if (years === null || rows.length === 0) return rows;
  const last = rows[rows.length - 1]!.month;
  const [y, m] = last.split("-").map(Number) as [number, number];
  const from = `${y - years}-${String(m).padStart(2, "0")}`;
  return rows.filter((r) => r.month > from);
}

function Axes({ max, fmt, months }: { max: number; fmt: (v: number) => string; months: string[] }) {
  const ticks = [0, 0.25, 0.5, 0.75, 1];
  const inner = H - PAD.t - PAD.b;
  const step = (W - PAD.l - PAD.r) / Math.max(1, months.length);
  return (
    <>
      {ticks.map((t) => {
        const y = PAD.t + inner * (1 - t);
        return (
          <g key={t}>
            <line x1={PAD.l} x2={W - PAD.r} y1={y} y2={y} stroke="var(--grid)" strokeWidth={1} />
            <text x={PAD.l - 6} y={y + 4} textAnchor="end">
              {fmt(max * t)}
            </text>
          </g>
        );
      })}
      {months.map((m, i) => {
        // Years on long ranges; every third month on short ones.
        const short = months.length <= 30;
        const show = short ? i % 3 === 0 : m.endsWith("-01");
        return show ? (
          <text key={m} x={PAD.l + step * (i + 0.5)} y={H - 6} textAnchor="middle">
            {short ? m : m.slice(0, 4)}
          </text>
        ) : null;
      })}
    </>
  );
}

export function BuySellChart({ flow, years = null }: { flow: FlowResponse; years?: number | null }) {
  const months = useMemo(() => inRange(monthlyFlow(flow), years), [flow, years]);
  if (months.length === 0) return <div className="empty">No trades in the database yet.</div>;
  const max = niceMax(Math.max(...months.map((m) => Math.max(m.buy, m.sell))));
  const inner = H - PAD.t - PAD.b;
  const step = (W - PAD.l - PAD.r) / months.length;
  const bw = Math.max(1, step * 0.4);
  return (
    <div className="chart">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Insider open-market buying and selling per month">
        <Axes max={max} fmt={money} months={months.map((m) => m.month)} />
        {months.map((m, i) => {
          const x = PAD.l + step * i + step * 0.1;
          const hb = (m.buy / max) * inner;
          const hs = (m.sell / max) * inner;
          return (
            <g key={m.month}>
              <rect x={x} y={PAD.t + inner - hb} width={bw} height={hb} fill="var(--buy)">
                <title>{`${m.month}: bought ${money(m.buy)}`}</title>
              </rect>
              <rect x={x + bw} y={PAD.t + inner - hs} width={bw} height={hs} fill="var(--sell)">
                <title>{`${m.month}: sold ${money(m.sell)}`}</title>
              </rect>
            </g>
          );
        })}
      </svg>
      <div className="legend">
        <span>
          <i className="sw" style={{ background: "var(--buy)" }} /> Open-market buys
        </span>
        <span>
          <i className="sw" style={{ background: "var(--sell)" }} /> Open-market sales
        </span>
      </div>
      <details className="table-view">
        <summary>Table</summary>
        <table>
          <thead>
            <tr>
              <th>Month</th>
              <th className="num">Bought</th>
              <th className="num">Sold</th>
            </tr>
          </thead>
          <tbody>
            {[...months].reverse().slice(0, 36).map((m) => (
              <tr key={m.month}>
                <td>{m.month}</td>
                <td className="num">{money(m.buy)}</td>
                <td className="num">{money(m.sell)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="muted">Latest 36 months.</div>
      </details>
    </div>
  );
}

export function EventsChart({ counts, years = null }: { counts: CountsResponse; years?: number | null }) {
  const months = useMemo(() => inRange(monthlyEvents(counts), years), [counts, years]);
  if (months.length === 0) return <div className="empty">No events yet. Run `oe events-build`.</div>;
  const totals = months.map((m) => [...m.row.values()].reduce((a, b) => a + b, 0));
  const max = niceMax(Math.max(...totals));
  const inner = H - PAD.t - PAD.b;
  const step = (W - PAD.l - PAD.r) / months.length;
  const bw = Math.max(1, step * 0.8);
  return (
    <div className="chart">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Events per month by type">
        <Axes max={max} fmt={count} months={months.map((m) => m.month)} />
        {months.map((m, i) => {
          let y = PAD.t + inner;
          return (
            <g key={m.month}>
              {RULES.map((r) => {
                const n = m.row.get(r.family) ?? 0;
                if (!n) return null;
                const h = (n / max) * inner;
                y -= h;
                return (
                  <rect key={r.family} x={PAD.l + step * i + step * 0.1} y={y} width={bw} height={h} fill={r.color}>
                    <title>{`${m.month}: ${n} ${r.label}`}</title>
                  </rect>
                );
              })}
            </g>
          );
        })}
      </svg>
      <div className="legend">
        {RULES.map((r) => (
          <span key={r.family}>
            <i className="sw" style={{ background: r.color }} /> {r.label}
          </span>
        ))}
      </div>
    </div>
  );
}
