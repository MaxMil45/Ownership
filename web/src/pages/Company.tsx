import { useEffect, useState } from "react";
import { findCompanies, getCompany, getEvents, getOfficialTrades, getPrices, getTrades, type CompanyResponse } from "../api";
import { OfficialTradeTable, ownerLabel, rangeLabel, sideLabel } from "./Congress";
import { BuySellChart } from "../charts";
import { InsightList, TradeExplanations } from "../ExplainBlocks";
import { summarizeCompany } from "../insights";
import { RangeSwitch } from "../RangeSwitch";
import { InsiderPicker } from "../InsiderPicker";
import { TradeChart } from "../TradeChart";
import { EventList } from "../EventList";
import { count, money, today } from "../format";
import { hashFor, navigate, type Route } from "../router";
import { ExternalLink, Loaded } from "../ui";
import { useAsync } from "../useAsync";

function roles(i: CompanyResponse["insiders"][number]): string {
  const r: string[] = [];
  if (i.director) r.push("director");
  if (i.officer) r.push(i.titles[0] ? `officer (${i.titles[0]})` : "officer");
  if (i.ten_percent_owner) r.push("10% owner");
  if (i.other) r.push("other");
  return r.join(", ") || "insider";
}

/** Type a name or ticker; pick a company. */
function CompanySearch() {
  const [q, setQ] = useState("");
  const [typed, setTyped] = useState("");
  // Ask once typing pauses, not on every key.
  useEffect(() => {
    const t = setTimeout(() => setTyped(q.trim()), 250);
    return () => clearTimeout(t);
  }, [q]);
  const found = useAsync((s) => (typed ? findCompanies(typed, s) : Promise.resolve(null)), [typed]);
  return (
    <div className="card" style={{ maxWidth: 640 }}>
      <label className="sub" htmlFor="company-q">
        Company name or ticker
      </label>
      <div>
        <input
          id="company-q"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          size={36}
          placeholder="Tesla, TSLA, Nvidia…"
          autoFocus
        />
      </div>
      {found.error && <div className="error">{found.error}</div>}
      {found.data && found.data.companies.length === 0 && (
        <div className="muted" style={{ marginTop: 8 }}>
          No company with events matches. Companies are found by name start or exact ticker.
        </div>
      )}
      <ul>
        {found.data?.companies.map((c) => (
          <li key={c.cik}>
            <a href={hashFor("/company", { cik: String(c.cik) })}>{c.name}</a>
            {c.ticker ? ` (${c.ticker})` : ""} <span className="muted">CIK {c.cik}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function Company({ route }: { route: Route }) {
  const cik = Number(route.params.cik ?? "");
  const valid = Number.isInteger(cik) && cik > 0;
  const company = useAsync((s) => (valid ? getCompany(cik, s) : Promise.resolve(null)), [cik]);
  const events = useAsync(
    (s) => (valid ? getEvents({ cik: String(cik), limit: 10 }, s) : Promise.resolve(null)),
    [cik],
  );
  const trades = useAsync((s) => (valid ? getTrades({ cik }, s) : Promise.resolve(null)), [cik]);
  // Market closes, if prices were imported and this company's series passed its check.
  const market = useAsync((s) => (valid ? getPrices(cik, s) : Promise.resolve(null)), [cik]);
  const members = useAsync(
    (s) => (valid ? getOfficialTrades({ cik: String(cik), limit: 200 }, s) : Promise.resolve(null)),
    [cik],
  );
  const [years, setYears] = useState<number | null>(5);
  const owner = Number(route.params.owner ?? "") || null;
  // Whose trades the chart marks: one person when the address names them.
  const [selected, setSelected] = useState<ReadonlySet<number> | null>(owner ? new Set([owner]) : null);
  const [fade, setFade] = useState(false);
  useEffect(() => setSelected(owner ? new Set([owner]) : null), [cik, owner]);
  if (!valid) return <CompanySearch />;
  return (
    <Loaded state={company}>
      {(c) =>
        c && (
          <>
            <div style={{ marginBottom: 12 }}>
              <h2 style={{ fontSize: 20 }}>
                {c.name}
                {c.ticker ? ` (${c.ticker})` : ""}
              </h2>
              <div className="sub">
                CIK {c.cik} · {count(c.filings)} insider filings, {c.first_on ?? "?"} to {c.last_on ?? "?"} ·{" "}
                <ExternalLink url={`https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK=${c.cik}`}>
                  SEC filings
                </ExternalLink>{" "}
                · <a href={hashFor("/company")}>Other company</a>
              </div>
            </div>

            <div className="card">
              <h2>What the filings show</h2>
              <Loaded state={trades}>
                {(t) => t && <InsightList lines={summarizeCompany(t.marks, today())} />}
              </Loaded>
              <div className="muted">
                Facts from insiders' own filings: what they bought and sold, not why.
              </div>
            </div>

            <div className="card">
              <div style={{ display: "flex", alignItems: "baseline", gap: 12, flexWrap: "wrap" }}>
                <div>
                  <h2>Insider trades on the price</h2>
                  <div className="sub">
                    Each ▲ is a buy and each ▼ a sale, at the price the insider reported. Hover for details,
                    click to open the filing.
                  </div>
                </div>
                <div style={{ marginLeft: "auto" }}>
                  <RangeSwitch years={years} set={setYears} />
                </div>
              </div>
              <Loaded state={trades}>
                {(t) =>
                  t && (
                    <>
                      <TradeChart
                        market={market.data?.days ?? null}
                        officials={(members.data?.trades ?? [])
                          .filter((m) => m.tx_date)
                          .map((m) => ({
                            day: m.tx_date!,
                            buy: m.tx_type === "P",
                            label: `${m.member} (${ownerLabel(m.owner).toLowerCase()}): ${sideLabel(m.tx_type).toLowerCase()} ${rangeLabel(m.amount_min, m.amount_max, m.amount)} on ${m.tx_date}`,
                            url: m.report_url,
                          }))}
                        marks={t.marks}
                        years={years}
                        selected={selected}
                        others={fade ? "fade" : "hide"}
                      />
                      {selected !== null && (
                        <label className="sub" style={{ display: "inline-flex", gap: 6, marginTop: 6 }}>
                          <input type="checkbox" checked={fade} onChange={(e) => setFade(e.target.checked)} />
                          Show everyone else's trades faded
                        </label>
                      )}
                      <InsiderPicker marks={t.marks} selected={selected} onChange={setSelected} />
                    </>
                  )
                }
              </Loaded>
              <TradeExplanations />
            </div>

            {members.data && members.data.total > 0 && (
              <div className="card">
                <h2>Members of Congress who traded it</h2>
                <div className="sub">
                  {count(members.data.total)} trades reported by House members, newest first (amounts are ranges).{" "}
                  <a href={hashFor("/congress", { ticker: c.ticker ?? undefined })}>All on the Congress page</a>
                </div>
                <OfficialTradeTable trades={members.data.trades.slice(0, 15)} />
              </div>
            )}

            <div className="card">
              <h2>Buying and selling per month</h2>
              <div className="sub">Open-market purchases and sales of this company's stock, in dollars.</div>
              <BuySellChart flow={c.flow} years={years} />
            </div>

            <div className="grid2">
              <div className="card">
                <h2>Insiders</h2>
                <div className="sub">Latest {c.insiders.length}, most recent filer first.</div>
                <table>
                  <thead>
                    <tr>
                      <th>Name</th>
                      <th>Role</th>
                      <th>Last filed</th>
                      <th className="num">Filings</th>
                    </tr>
                  </thead>
                  <tbody>
                    {c.insiders.map((i) => (
                      <tr key={i.owner_cik}>
                        <td>
                          <a href={hashFor("/lookup", { name: i.name })}>{i.name}</a>{" "}
                          <a
                            className="muted"
                            href={hashFor("/company", { cik: String(cik), owner: String(i.owner_cik) })}
                            title="Show only this insider's trades on the chart"
                          >
                            (chart)
                          </a>
                        </td>
                        <td>{roles(i)}</td>
                        <td>{i.last_on}</td>
                        <td className="num">{count(i.filings)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="card">
                <h2>Latest open-market trades</h2>
                <table>
                  <thead>
                    <tr>
                      <th>Date</th>
                      <th>Who</th>
                      <th>Trade</th>
                      <th className="num">Value</th>
                    </tr>
                  </thead>
                  <tbody>
                    {c.trades.map((t, n) => (
                      <tr key={`${t.receipt.accession}-${n}`}>
                        <td>{t.transaction_date}</td>
                        <td>{t.owner_name}</td>
                        <td>
                          <ExternalLink url={t.receipt.url}>
                            {t.code === "P" ? "bought" : "sold"} {t.shares !== null ? count(t.shares) : "?"}
                          </ExternalLink>
                        </td>
                        <td className="num">{t.value !== null ? money(t.value) : "?"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {c.trades.length === 0 && <div className="empty">No open-market trades.</div>}
              </div>
            </div>

            <div className="card">
              <h2>Events</h2>
              <Loaded state={events}>
                {(e) =>
                  e && (
                    <>
                      <EventList events={e.events} />
                      {e.total > e.events.length && (
                        <p>
                          <button type="button" onClick={() => navigate("/events", { cik: String(cik) })}>
                            All {count(e.total)} events
                          </button>
                        </p>
                      )}
                    </>
                  )
                }
              </Loaded>
            </div>
          </>
        )
      }
    </Loaded>
  );
}
