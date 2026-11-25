import { useEffect, useState } from "react";
import { lookup, type Candidate } from "../api";
import { count, money, percent } from "../format";
import { hashFor, navigate, type Route } from "../router";
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

function CandidateCard({ c }: { c: Candidate }) {
  return (
    <div className="card candidate">
      <h3>{c.display_name}</h3>
      <div className="sub">
        {c.kinds.map((k) => (
          <span className="tag" key={k}>
            {k}
          </span>
        ))}
        {count(c.filings)} filing{c.filings === 1 ? "" : "s"}, {c.first_on ?? "?"} to {c.last_on ?? "?"} ·{" "}
        <span className="muted">{c.key}</span>
        {!c.exact && <span className="muted"> · partial name match</span>}
      </div>
      {c.other_names.length > 0 && (
        <div className="muted">Also filed as: {c.other_names.slice(0, 4).join("; ")}</div>
      )}

      {c.roles.length > 0 && (
        <>
          <h2>Roles</h2>
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
          <h2>Schedule 13D/13G</h2>
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
          <h2>Contested proxies</h2>
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
  const [draft, setDraft] = useState(name);
  useEffect(() => setDraft(name), [name]);
  const state = useAsync((s) => (name ? lookup(name, s) : Promise.resolve(null)), [name]);

  return (
    <>
      <form
        className="filters"
        onSubmit={(e) => {
          e.preventDefault();
          navigate("/lookup", { name: draft.trim() || undefined });
        }}
      >
        <label>
          Who is this name?
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            size={36}
            placeholder="Elon Musk, Elliott Investment Management, Nancy Pelosi"
            autoFocus
          />
        </label>
        <button className="primary" type="submit">
          Look up
        </button>
      </form>

      {!name && (
        <div className="notice">
          Type a person's or a fund's name. Every match is its own candidate with its filings: two people
          with the same name are never merged. A name with no match says nothing about the person.
        </div>
      )}
      {state.error && <div className="error">{state.error}</div>}
      {name && state.loading && !state.data && <div className="muted">Looking up…</div>}
      {state.data && state.data.result !== "found" && <div className="notice">{state.data.message}</div>}
      {state.data && state.data.result === "found" && state.data.candidates.length === 0 && (
        <div className="notice">{state.data.message}</div>
      )}
      {state.data?.candidates.map((c) => <CandidateCard key={c.key} c={c} />)}
    </>
  );
}
