import type { EventItem } from "./api";
import { money, percent, ruleColor, ruleLabel } from "./format";
import { explainRule } from "./explain";
import { hashFor } from "./router";
import { ExternalLink } from "./ui";

/** Events as a list; each opens to its details and SEC receipts. */
export function EventList({ events }: { events: EventItem[] }) {
  if (events.length === 0) return <div className="empty">No events match.</div>;
  return (
    <div className="events">
      {events.map((e) => (
        <details className="ev" key={e.key}>
          <summary>
            <span className="muted">{e.known_on ?? "?"}</span>
            <span className="rule">
              <i className="sw" style={{ background: ruleColor(e.rule_id) }} />
              {ruleLabel(e.rule_id)}
            </span>
            <span>{e.headline}</span>
            <span className="amount">
              {e.amount_usd !== null ? money(e.amount_usd) : e.percent !== null ? percent(e.percent) : ""}
            </span>
          </summary>
          <div className="body">
            {e.details.map((d, i) => (
              <p key={i}>{d}</p>
            ))}
            {explainRule(e.rule_id) && (
              <details className="explain">
                <summary>What does this mean?</summary>
                <p>{explainRule(e.rule_id)!.text}</p>
              </details>
            )}
            {e.people.length > 0 && (
              <p>
                {e.people.slice(0, 8).map((p, i) => (
                  <span key={`${p.name}-${i}`}>
                    {i > 0 ? ", " : "Who: "}
                    <a href={hashFor("/lookup", { name: p.name })}>{p.name}</a>
                  </span>
                ))}
                {e.people.length > 8 ? ` and ${e.people.length - 8} more` : ""}
              </p>
            )}
            {e.issuer_cik !== null && (
              <p>
                <a href={hashFor("/company", { cik: String(e.issuer_cik) })}>
                  {e.issuer_name ?? e.ticker ?? `CIK ${e.issuer_cik}`}: company page
                </a>
                {e.ticker && (
                  <>
                    {" · "}
                    <a href={hashFor("/events", { ticker: e.ticker })}>all events for {e.ticker}</a>
                  </>
                )}
              </p>
            )}
            <p>
              {e.receipts.map((r) => (
                <span key={r.accession} style={{ marginRight: 12 }}>
                  <ExternalLink url={r.url}>
                    {r.form ? `Form ${r.form} ` : ""}
                    {r.accession}
                  </ExternalLink>
                </span>
              ))}
            </p>
          </div>
        </details>
      ))}
    </div>
  );
}
