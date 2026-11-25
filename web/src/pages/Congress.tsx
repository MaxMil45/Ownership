import { useEffect, useState } from "react";
import { getActiveMembers, getOfficialTrades, type OfficialTrade } from "../api";
import { count, daysBefore, money, today } from "../format";
import { hashFor, type Route } from "../router";
import { ExternalLink, Loaded } from "../ui";
import { useAsync } from "../useAsync";

const OWNER: Record<string, string> = { SP: "Spouse", JT: "Joint", DC: "Child" };
const PAGE = 50;

export function ownerLabel(code: string | null): string {
  return code ? (OWNER[code] ?? code) : "Self";
}

export function sideLabel(t: string): string {
  return t === "P" ? "Buy" : t === "E" ? "Exchange" : t === "S (partial)" ? "Partial sale" : "Sale";
}

export function rangeLabel(min: number | null, max: number | null, written: string): string {
  if (min === null) return written || "?";
  if (max === null) return `over ${money(min - 1)}`;
  return min === max ? money(min) : `${money(min)}–${money(max)}`;
}

/** Trades as a table; `member` false hides the member column (one member's page). */
export function OfficialTradeTable({ trades, member = true }: { trades: OfficialTrade[]; member?: boolean }) {
  if (trades.length === 0) return <div className="empty">No trades match.</div>;
  return (
    <table>
      <thead>
        <tr>
          <th>Traded</th>
          {member && <th>Member</th>}
          <th>Whose</th>
          <th>Trade</th>
          <th>Asset</th>
          <th className="num">Amount</th>
          <th>Reported</th>
        </tr>
      </thead>
      <tbody>
        {trades.map((t) => (
          <tr key={`${t.doc_id}-${t.line_no}`}>
            <td>{t.tx_date ?? "?"}</td>
            {member && (
              <td>
                <a href={hashFor("/congress", { bioguide: t.bioguide ?? undefined })}>{t.member}</a>
              </td>
            )}
            <td>{ownerLabel(t.owner)}</td>
            <td className={t.tx_type === "P" ? "buy-text" : t.tx_type.startsWith("S") ? "sell-text" : ""}>
              {sideLabel(t.tx_type)}
            </td>
            <td>
              {t.issuer_cik ? (
                <a href={hashFor("/company", { cik: String(t.issuer_cik) })}>{t.asset}</a>
              ) : (
                t.asset
              )}
              {t.description && <div className="muted">{t.description}</div>}
            </td>
            <td className="num">{rangeLabel(t.amount_min, t.amount_max, t.amount)}</td>
            <td>
              <ExternalLink url={t.report_url}>{t.filed_on ?? "report"}</ExternalLink>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Active({ onPick }: { onPick: (b: string) => void }) {
  const [year, setYear] = useState(true);
  const since = year ? daysBefore(today(), 365) : null;
  const a = useAsync((s) => getActiveMembers(since, s), [since]);
  return (
    <div className="card">
      <div style={{ display: "flex", gap: 12, alignItems: "baseline", flexWrap: "wrap" }}>
        <h2>Most active traders</h2>
        <div className="seg" role="group" aria-label="Period">
          <button type="button" aria-pressed={year} onClick={() => setYear(true)}>
            Last 12 months
          </button>
          <button type="button" aria-pressed={!year} onClick={() => setYear(false)}>
            Since 2014
          </button>
        </div>
      </div>
      <Loaded state={a}>
        {(d) => (
          <table>
            <thead>
              <tr>
                <th>Member</th>
                <th className="num">Trades</th>
                <th className="num">Buys</th>
                <th className="num">Sales</th>
                <th className="num">Reported value</th>
                <th>Most traded</th>
                <th>Latest</th>
              </tr>
            </thead>
            <tbody>
              {d.members.map((m) => (
                <tr key={m.bioguide}>
                  <td>
                    <button type="button" className="linkish" onClick={() => onPick(m.bioguide)}>
                      {m.name}
                    </button>
                  </td>
                  <td className="num">{count(m.trades)}</td>
                  <td className="num">{count(m.buys)}</td>
                  <td className="num">{count(m.sales)}</td>
                  <td className="num">
                    {money(m.low_total)}–{money(m.high_total)}
                  </td>
                  <td>{m.top_tickers.join(", ")}</td>
                  <td>{m.last_trade ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Loaded>
    </div>
  );
}

export function Congress({ route }: { route: Route }) {
  const p = route.params;
  const page = Math.max(0, Number(p.page ?? "0") || 0);
  const [ticker, setTicker] = useState(p.ticker ?? "");
  useEffect(() => setTicker(p.ticker ?? ""), [p.ticker]);
  const filter = {
    bioguide: p.bioguide || undefined,
    ticker: p.ticker || undefined,
    side: p.side || undefined,
    limit: PAGE,
    offset: page * PAGE,
  };
  const trades = useAsync((s) => getOfficialTrades(filter, s), [JSON.stringify(filter)]);
  const go = (next: Record<string, string | undefined>) =>
    (window.location.hash = hashFor("/congress", { bioguide: p.bioguide, ticker: p.ticker, side: p.side, ...next, page: next.page }));

  return (
    <>
      <div className="notice">
        Members of the House report each stock trade above $1,000 within 45 days, in ranges (for example
        $1,001–$15,000), not exact amounts, including their spouse's and children's. Read from the reports the
        House Clerk publishes; scanned paper reports are listed in Lookup but not read. These reports may not be
        used commercially (5 U.S.C. 13107). Senators are not included yet.
      </div>
      {!p.bioguide && !p.ticker && <Active onPick={(b) => go({ bioguide: b })} />}

      <div className="card">
        <form
          className="filters"
          onSubmit={(e) => {
            e.preventDefault();
            go({ ticker: ticker.trim() || undefined });
          }}
        >
          <label>
            Ticker
            <input value={ticker} onChange={(e) => setTicker(e.target.value)} size={8} placeholder="NVDA" />
          </label>
          <div className="seg" role="group" aria-label="Buys or sales">
            {[
              ["", "All"],
              ["buy", "Buys"],
              ["sale", "Sales"],
            ].map(([v, l]) => (
              <button key={v} type="button" aria-pressed={(p.side ?? "") === v} onClick={() => go({ side: v || undefined })}>
                {l}
              </button>
            ))}
          </div>
          <button className="primary" type="submit">
            Search
          </button>
          {(p.bioguide || p.ticker || p.side) && (
            <button type="button" onClick={() => (window.location.hash = hashFor("/congress"))}>
              Everyone
            </button>
          )}
        </form>
        <Loaded state={trades}>
          {(t) => (
            <>
              <h2>
                {p.bioguide && t.trades[0] ? (
                  <>
                    {t.trades[0].member}'s trades ·{" "}
                    <a className="muted" href={hashFor("/lookup", { name: t.trades[0].member })}>
                      profile
                    </a>
                  </>
                ) : (
                  "Latest trades"
                )}
              </h2>
              <div className="sub">{count(t.total)} trades, newest first</div>
              <OfficialTradeTable trades={t.trades} member={!p.bioguide} />
              <div className="pager">
                <button type="button" disabled={page === 0} onClick={() => go({ page: page > 1 ? String(page - 1) : undefined })}>
                  Newer
                </button>
                <span className="muted">
                  {t.total === 0 ? "0" : `${page * PAGE + 1}–${Math.min(t.total, page * PAGE + t.trades.length)}`} of {count(t.total)}
                </span>
                <button type="button" disabled={(page + 1) * PAGE >= t.total} onClick={() => go({ page: String(page + 1) })}>
                  Older
                </button>
              </div>
            </>
          )}
        </Loaded>
      </div>
    </>
  );
}
