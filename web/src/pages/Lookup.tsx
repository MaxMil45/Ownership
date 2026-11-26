import { useMemo, useState } from "react";
import { getOfficialTrades, getOutcomes, getPrices, getTrades, lookup, type Candidate } from "../api";
import { OfficialTradeTable } from "./Congress";
import { inShort, kindLabel, oneLine } from "../describe";
import { Explain, InsightList } from "../ExplainBlocks";
import { summarizeOfficial, summarizePerson, trackRecord } from "../insights";
import { RangeSwitch } from "../RangeSwitch";
import { TradeChart } from "../TradeChart";
import { count, money, percent, today } from "../format";
import { hashFor, type Route } from "../router";
import { ExternalLink } from "../ui";
import { useAsync } from "../useAsync";

function roleText(r: Candidate["roles"][number]): string {
  const parts: string[] = [];
  if (r.director) parts.push("director");
  if (r.officer) parts.push(r.titles[0] ? `officer (${r.titles[0]})` : "officer");
  if (r.ten_percent_owner) parts.push("10% owner");
  if (r.other) parts.push("other");
  return parts.length ? parts.join(", ") : "insider";
}

/** A member of Congress's trades, read from their House reports. */
function MemberTrades({ bioguide }: { bioguide: string }) {
  const t = useAsync((s) => getOfficialTrades({ bioguide, limit: 500 }, s), [bioguide]);
  if (t.error) return <div className="error">{t.error}</div>;
  if (!t.data) return <div className="muted">Reading their trades...</div>;
  if (t.data.total === 0) return <div className="muted">No transactions read from their reports.</div>;
  return (
    <>
      <h2>Their trades</h2>
      <InsightList lines={summarizeOfficial(t.data.trades)} />
      <OfficialTradeTable trades={t.data.trades.slice(0, 10)} member={false} />
      <p>
        <a href={hashFor("/congress", { bioguide })}>All {count(t.data.total)} trades</a>
      </p>
    </>
  );
}

/** One person's trades at one company, on the price everyone traded at. */
function PersonChart({ issuer, owner }: { issuer: number; owner: number }) {
  const [open, setOpen] = useState(false);
  const [others, setOthers] = useState(false);
  const [years, setYears] = useState<number | null>(null);
  const only = useMemo(() => new Set([owner]), [owner]);
  // Fetched only once someone opens it: a company's trades can be large.
  const t = useAsync((s) => (open ? getTrades({ cik: issuer }, s) : Promise.resolve(null)), [open, issuer]);
  const market = useAsync((s) => (open ? getPrices(issuer, s) : Promise.resolve(null)), [open, issuer]);
  return (
    <details className="person-chart" onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}>
      <summary>Show on the price chart</summary>
      {t.error && <div className="error">{t.error}</div>}
      {open && !t.data && !t.error && <div className="muted">Loading the company's trades…</div>}
      {t.data && (
        <>
          <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap", margin: "6px 0" }}>
            <RangeSwitch years={years} set={setYears} />
            <label className="sub" style={{ display: "inline-flex", gap: 6 }}>
              <input type="checkbox" checked={others} onChange={(e) => setOthers(e.target.checked)} />
              Also show the other insiders' trades (faded)
            </label>
          </div>
          <TradeChart
            marks={t.data.marks}
            market={market.data?.days ?? null}
            years={years}
            selected={only}
            others={others ? "fade" : "hide"}
          />
          <div className="muted">
            The line is the price all insiders of the company traded at, so you can see what the stock was doing
            around each of this person's trades.
          </div>
        </>
      )}
    </details>
  );
}

/** An insider's buying and selling pattern at each company. */
function Patterns({ cik }: { cik: number }) {
  const t = useAsync((s) => getTrades({ owner: cik }, s), [cik]);
  // Their record, when prices are imported (an empty list otherwise).
  const record = useAsync((s) => getOutcomes({ owner: cik }, s), [cik]);
  if (t.error) return <div className="error">{t.error}</div>;
  if (!t.data) return <div className="muted">Reading their trades…</div>;
  const people = summarizePerson(t.data.marks, today());
  if (people.length === 0) return null;
  return (
    <>
      <h2>Buying and selling, company by company</h2>
      {people.map((p) => (
        <div key={p.issuer_cik} className="pattern">
          <div>
            <strong>{p.label}</strong> ·{" "}
            <a href={hashFor("/company", { cik: String(p.issuer_cik), owner: String(cik) })}>
              see their trades on the chart
            </a>
          </div>
          <InsightList
            lines={(() => {
              const r = record.data ? trackRecord(record.data.outcomes, p.issuer_cik) : null;
              return r ? [...p.lines, r] : p.lines;
            })()}
          />
          <PersonChart issuer={p.issuer_cik} owner={cik} />
        </div>
      ))}
      <div className="muted">From their own filings: what they did, not why.</div>
      <div className="explain-row">
        <Explain topic="buy" title="What does a buy mean?" />
        <Explain topic="sell" title="What does a sale mean?" />
      </div>
    </>
  );
}

function CandidateCard({ c }: { c: Candidate }) {
  const insiderCik = c.key.startsWith("cik:") && c.kinds.includes("insider") ? Number(c.key.slice(4)) : null;
  return (
    <div className="card candidate">
      <h3>{c.display_name}</h3>
      <div className="sub">
        {c.kinds.map((k) => (
          <span className={`tag kind-${k}`} key={k}>
            {kindLabel(k)}
          </span>
        ))}
        {count(c.filings)} filing{c.filings === 1 ? "" : "s"}, {c.first_on ?? "?"} to {c.last_on ?? "?"} ·{" "}
        <span className="muted">{c.key.startsWith("cik:") ? `SEC CIK ${c.key.slice(4)}` : c.key.startsWith("bioguide:") ? `Bioguide ${c.key.slice(9)}` : "no SEC number (named in 13D/G filings)"}</span>
        {!c.exact && <span className="muted"> · partial name match</span>}
      </div>
      {c.other_names.length > 0 && (
        <div className="muted">Also filed as: {c.other_names.slice(0, 4).join("; ")}</div>
      )}
      <div className="in-short">
        <h2>In short</h2>
        {inShort(c).map((line, i) => (
          <p key={i}>{line}</p>
        ))}
      </div>

      {c.roles.length > 0 && (
        <>
          <h2>Roles in companies</h2>
          <ul>
            {c.roles.map((r) => (
              <li key={r.issuer_cik}>
                {roleText(r)} at{" "}
                <a href={hashFor("/company", { cik: String(r.issuer_cik) })}>{r.issuer_name}</a>
                {r.ticker ? ` (${r.ticker})` : ""}, {r.first_on} to {r.last_on}, {r.filings} filing
                {r.filings === 1 ? "" : "s"}
              </li>
            ))}
          </ul>
        </>
      )}

      {insiderCik !== null && <Patterns cik={insiderCik} />}

      {c.trades.length > 0 && (
        <>
          <h2>Latest open-market trades</h2>
          <ul>
            {c.trades.map((t, i) => (
              <li key={i}>
                {t.transaction_date} {t.code === "P" ? "bought" : "sold"}{" "}
                {t.shares !== null ? count(t.shares) : "?"} shares of {t.ticker ?? t.issuer_name}
                {t.value !== null ? ` for ${money(t.value)}` : ""}; filed {t.receipt.known_on}
                {t.receipt.filing_date_only ? " (date only)" : ""} ·{" "}
                <ExternalLink url={t.receipt.url}>SEC filing</ExternalLink>
              </li>
            ))}
          </ul>
        </>
      )}

      {c.holdings.length > 0 && (
        <>
          <h2>Stakes over 5% (Schedule 13D/13G)</h2>
          <ul>
            {c.holdings.map((h) => (
              <li key={h.receipt.accession}>
                {h.submission_type} of{" "}
                <a href={hashFor("/company", { cik: String(h.issuer_cik) })}>{h.issuer_name}</a> ({h.class_title})
                {h.percent !== null ? `, ${percent(h.percent)}` : ""}; filed {h.receipt.known_on} ·{" "}
                <ExternalLink url={h.receipt.url}>SEC filing</ExternalLink>
              </li>
            ))}
          </ul>
        </>
      )}

      {c.contests.length > 0 && (
        <>
          <h2>Proxy fights (contested proxy filings)</h2>
          <ul>
            {c.contests.map((k) => (
              <li key={k.receipt.accession}>
                {k.form} about {k.subject_name}; filed {k.receipt.known_on} ·{" "}
                <ExternalLink url={k.receipt.url}>SEC filing</ExternalLink>
              </li>
            ))}
          </ul>
        </>
      )}

      {c.official && (
        <>
          <h2>Member of Congress</h2>
          <ul>
            {c.official.terms.slice(0, 3).map((t) => (
              <li key={t}>{t}</li>
            ))}
          </ul>
          {c.official.committees.length > 0 && (
            <div className="sub">
              Committees: {c.official.committees.map((m) => (m.title ? `${m.name} (${m.title})` : m.name)).join("; ")}
            </div>
          )}
          <MemberTrades bioguide={c.official.bioguide} />
          {c.official.reports > 0 && (
            <>
              <p className="sub">
                {count(c.official.reports)} House periodic transaction report
                {c.official.reports === 1 ? "" : "s"}, {c.official.first_filed ?? "?"} to{" "}
                {c.official.last_filed ?? "?"}
              </p>
              <ul>
                {c.official.recent_reports.slice(0, 5).map((r) => (
                  <li key={r.doc_id}>
                    {r.filed_on}
                    {r.scanned ? " (scanned paper)" : ""} · <ExternalLink url={r.url}>Report PDF</ExternalLink>
                  </li>
                ))}
              </ul>
            </>
          )}
          <div className="muted">{c.official.use_limits}</div>
        </>
      )}
    </div>
  );
}

export function Lookup({ route }: { route: Route }) {
  const name = (route.params.name ?? "").trim();
  const key = route.params.key ?? "";
  const state = useAsync((s) => (name ? lookup(name, s) : Promise.resolve(null)), [name]);
  const all = state.data?.result === "found" ? state.data.candidates : [];
  const picked = key ? all.filter((c) => c.key === key) : all.length === 1 ? all : [];

  return (
    <>
      {!name && (
        <div className="notice">
          Search for a person or a fund with the box at the top. Every match is its own candidate with its own
          filings: two people with the same name are never merged.
        </div>
      )}
      {state.error && <div className="error">{state.error}</div>}
      {name && state.loading && !state.data && <div className="muted">Looking up…</div>}
      {state.data && state.data.result !== "found" && <div className="notice">{state.data.message}</div>}
      {state.data && state.data.result === "found" && all.length === 0 && (
        <div className="notice">{state.data.message}</div>
      )}
      {key && state.data && all.length > 0 && picked.length === 0 && (
        <div className="notice">That match is no longer in the results for “{name}”. Pick one below.</div>
      )}
      {picked.length === 0 && all.length > 1 && (
        <div className="card">
          <h2>
            {count(all.length)} matches for “{name}”: pick one
          </h2>
          <ul className="result-list">
            {all.map((c) => (
              <li className="result" key={c.key}>
                <a className="result-name" href={hashFor("/lookup", { name, key: c.key })}>
                  {c.display_name}
                </a>
                <span className="result-line">{oneLine(c)}</span>
                <span className="result-meta">
                  {c.kinds.map((k) => (
                    <span className={`tag kind-${k}`} key={k}>
                      {kindLabel(k)}
                    </span>
                  ))}
                  {count(c.filings)} filings{c.first_on && c.last_on ? `, ${c.first_on.slice(0, 4)}–${c.last_on.slice(0, 4)}` : ""}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {picked.map((c) => (
        <CandidateCard key={c.key} c={c} />
      ))}
      {picked.length === 1 && all.length > 1 && (
        <p>
          <a href={hashFor("/lookup", { name })}>← The other {count(all.length - 1)} matches for “{name}”</a>
        </p>
      )}
    </>
  );
}
