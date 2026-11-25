import { useState } from "react";
import { getExtremes, getHistory, getInsiderRecords, type Horizon, type Outcome } from "../api";
import { count, ruleLabel } from "../format";
import { hashFor } from "../router";
import { Loaded } from "../ui";
import { useAsync } from "../useAsync";

const BASELINE = "baseline.any_month";
const FAMILIES: { family: string; label: string }[] = [
  { family: BASELINE, label: "Any company, any month (the yardstick)" },
  { family: "official.buy", label: "A member of Congress's buy" },
  { family: "official.sale", label: "A member of Congress's sale" },
  { family: "insider.buy", label: "An insider's open-market buy" },
  { family: "insider.sale", label: "An insider's open-market sale" },
  { family: "insider.cluster_buy", label: "Cluster buy (3+ insiders)" },
  { family: "insider.planned_sale", label: "Planned sale (Form 144)" },
  { family: "insider.plan_terminated", label: "10b5-1 plan ended" },
  { family: "activist.new_13d", label: "New 13D (activist over 5%)" },
  { family: "activist.passive_to_active", label: "Passive to active" },
  { family: "activist.proxy_contest", label: "Proxy contest" },
];
const labelOf = (f: string) => FAMILIES.find((x) => x.family === f)?.label ?? ruleLabel(f);

/** `0.123` -> `+12.3%`; null -> `–`. */
export function signedPct(v: number | null | undefined, digits = 1): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return "–";
  const text = (v * 100).toFixed(digits);
  if (Number(text) === 0) return `${(0).toFixed(digits)}%`;
  return `${v > 0 ? "+" : ""}${text}%`;
}
const share = (v: number | null) => (v === null ? "–" : `${Math.round(v * 100)}%`);
const at = (hs: Horizon[], days: number) => hs.find((h) => h.days === days);

function Caveats() {
  return (
    <div className="notice">
      <strong>History, not advice.</strong> These are past results, measured from the first market close after
      each filing became public (what someone reading it could have done), against SPY, an S&amp;P 500 fund, over
      the same days. They describe what happened, and say nothing certain about what will happen. Companies that
      went bankrupt or were bought out often have no prices in the file and are missing, which makes the results
      look better than they were. A median of many cases means more than any single one. A typical single
      stock trails the index in most years (a few very large winners lift the index), so read each row
      against the first one, "any company, any month", not against zero.
    </div>
  );
}

function Bar({ v, max }: { v: number | null; max: number }) {
  if (v === null || max <= 0) return null;
  const w = Math.min(100, (Math.abs(v) / max) * 100);
  return (
    <div className="hbar" aria-hidden="true">
      <div className="hbar-mid" />
      <div
        className="hbar-fill"
        style={{
          width: `${w / 2}%`,
          left: v >= 0 ? "50%" : `${50 - w / 2}%`,
          background: v >= 0 ? "var(--buy)" : "var(--sell)",
        }}
      />
    </div>
  );
}

function ExtremesTable({ family }: { family: string }) {
  const best = useAsync((s) => getExtremes(family, true, s), [family]);
  const worst = useAsync((s) => getExtremes(family, false, s), [family]);
  const rows = (o: Outcome[]) => (
    <table>
      <thead>
        <tr>
          <th>Public on</th>
          <th>Company</th>
          <th className="num">12 months</th>
          <th className="num">vs market</th>
        </tr>
      </thead>
      <tbody>
        {o.map((x) => (
          <tr key={x.ref}>
            <td>{x.known_on}</td>
            <td>
              <a href={hashFor("/company", { cik: String(x.issuer_cik) })}>{x.issuer_name || `CIK ${x.issuer_cik}`}</a>
              {x.ticker ? ` (${x.ticker})` : ""}
            </td>
            <td className="num">{signedPct(x.r[3] ?? null, 0)}</td>
            <td className="num">{signedPct(x.x[3] ?? null, 0)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
  return (
    <div className="grid2">
      <div className="card">
        <h2>Rose most a year later</h2>
        <Loaded state={best}>{(b) => (b.outcomes.length ? rows(b.outcomes) : <div className="empty">None measured yet.</div>)}</Loaded>
      </div>
      <div className="card">
        <h2>Fell most a year later</h2>
        <Loaded state={worst}>{(w) => (w.outcomes.length ? rows(w.outcomes) : <div className="empty">None measured yet.</div>)}</Loaded>
      </div>
    </div>
  );
}

function RecordTable({ best }: { best: boolean }) {
  const r = useAsync((s) => getInsiderRecords(best, s), [best]);
  return (
    <Loaded state={r}>
      {(d) =>
        d.insiders.length === 0 ? (
          <div className="empty">No insider has three measured buys yet.</div>
        ) : (
          <table>
            <thead>
              <tr>
                <th>Insider</th>
                <th className="num">Buys measured</th>
                <th className="num">Median a year later</th>
                <th className="num">vs market</th>
                <th className="num">Beat the market</th>
                <th>Last buy</th>
              </tr>
            </thead>
            <tbody>
              {d.insiders.map((i) => (
                <tr key={i.owner_cik}>
                  <td>
                    <a href={hashFor("/lookup", { name: i.name })}>{i.name || `CIK ${i.owner_cik}`}</a>
                    {i.companies > 1 ? <span className="muted"> · {i.companies} companies</span> : null}
                  </td>
                  <td className="num">{count(i.measured)}</td>
                  <td className="num">{signedPct(i.median_return_12m, 0)}</td>
                  <td className="num">{signedPct(i.median_excess_12m, 0)}</td>
                  <td className="num">{share(i.share_beat_12m)}</td>
                  <td>{i.last_buy ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )
      }
    </Loaded>
  );
}

export function History() {
  const h = useAsync((s) => getHistory(s), []);
  const [family, setFamily] = useState("insider.cluster_buy");
  return (
    <Loaded state={h}>
      {(d) => {
        if (d.prices.series_ok === 0) {
          return (
            <div className="card" style={{ maxWidth: 760 }}>
              <h2>What happened after? Needs prices first</h2>
              <p className="sub">
                This page shows what companies' shares did in the months after insiders bought or sold and after
                each kind of event. That needs daily market prices, which do not come with the project.
              </p>
              <ol className="sub">
                <li>
                  On <strong>stooq.com</strong>, open Database, choose daily → U.S. → ASCII, and save the zip into{" "}
                  <code>data\prices\</code> (you accept Stooq's terms; it stays on this computer).
                </li>
                <li>
                  In the project folder run <code>.\target\release\oe.exe prices-import data\prices\&lt;the zip&gt;</code>{" "}
                  (a few minutes).
                </li>
                <li>Open this page again.</li>
              </ol>
              <Caveats />
            </div>
          );
        }
        const groups = FAMILIES.map((f) => d.groups.find((g) => g.family === f.family)).filter(
          (g): g is NonNullable<typeof g> => !!g,
        );
        const base = at(d.groups.find((g) => g.family === BASELINE)?.horizons ?? [], 252)?.median_excess ?? null;
        // Each kind's 12-month median against the yardstick's.
        const rel = (g: (typeof groups)[number]): number | null => {
          const m = at(g.horizons, 252)?.median_excess;
          return m === null || m === undefined || base === null ? null : m - base;
        };
        const maxX = Math.max(0.01, ...groups.map((g) => Math.abs(rel(g) ?? 0)));
        return (
          <>
            <Caveats />
            <div className="card">
              <h2>What happened after each kind of filing</h2>
              <div className="sub">
                Median share price change 3 and 12 months after the filing became public, and against the market.
                Prices for {count(d.prices.series_ok)} companies
                {d.prices.last_day ? ` to ${d.prices.last_day}` : ""}, each checked against what its insiders
                reported paying ({count(d.prices.series_mismatch + d.prices.series_unchecked)} left out).
              </div>
              <table className="history-table">
                <thead>
                  <tr>
                    <th>After…</th>
                    <th className="num">Cases</th>
                    <th className="num">3 months</th>
                    <th className="num">12 months</th>
                    <th className="num">12 m vs market</th>
                    <th className="num">average</th>
                    <th className="num">Beat the market</th>
                    <th style={{ width: "20%" }}>vs the yardstick</th>
                  </tr>
                </thead>
                <tbody>
                  {groups.map((g) => {
                    const m3 = at(g.horizons, 63);
                    const y1 = at(g.horizons, 252);
                    return (
                      <tr key={g.family} className={g.family === BASELINE ? "yardstick" : undefined}>
                        <td>
                          {g.family === BASELINE ? (
                            <strong>{labelOf(g.family)}</strong>
                          ) : (
                            <button type="button" className="linkish" onClick={() => setFamily(g.family)}>
                              {labelOf(g.family)}
                            </button>
                          )}
                        </td>
                        <td className="num">
                          {count(y1?.n ?? 0)}
                          <span className="muted"> / {count(g.count)}</span>
                        </td>
                        <td className="num">{signedPct(m3?.median_return)}</td>
                        <td className="num">{signedPct(y1?.median_return)}</td>
                        <td className="num">{signedPct(y1?.median_excess)}</td>
                        <td className="num">{signedPct(y1?.mean_excess)}</td>
                        <td className="num">{share(y1?.share_beat ?? null)}</td>
                        <td>
                          {g.family === BASELINE ? (
                            <span className="muted">the yardstick</span>
                          ) : (
                            <span title={`${signedPct(rel(g))} against the yardstick's median`}>
                              <Bar v={rel(g)} max={maxX} />
                            </span>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              <div className="muted">
                Cases: measured a year on / all (the rest are too recent). "vs market": the change minus SPY's over the
                same days, median and average (a few big winners pull averages up). The bar: how much better (right)
                or worse (left) each kind's median did than the yardstick's. Click a row for its biggest moves below.
              </div>
            </div>

            <h2 style={{ margin: "18px 0 8px" }}>After: {labelOf(family)}</h2>
            <ExtremesTable family={family} />

            <div className="card">
              <h2>Insiders whose buys did best afterwards</h2>
              <div className="sub">
                At least three open-market buys a year old or more, ranked by the median change against the market
                in the year after each buy became public.
              </div>
              <RecordTable best={true} />
            </div>
            <div className="card">
              <h2>Insiders whose buys did worst afterwards</h2>
              <RecordTable best={false} />
            </div>
          </>
        );
      }}
    </Loaded>
  );
}
