import { useEffect, useState } from "react";
import { getEvents, type EventFilter } from "../api";
import { EventList } from "../EventList";
import { count, RULES } from "../format";
import { hashFor, type Route } from "../router";
import { Loaded } from "../ui";
import { useAsync } from "../useAsync";

const PAGE = 50;
const FIELDS = ["rule", "ticker", "cik", "q", "since", "until", "min_amount"] as const;
type Field = (typeof FIELDS)[number];

function fromParams(params: Record<string, string>): Record<Field, string> {
  const out = {} as Record<Field, string>;
  for (const f of FIELDS) out[f] = params[f] ?? "";
  return out;
}

export function Events({ route }: { route: Route }) {
  // The address holds the filters, so a page can be bookmarked or shared.
  const applied = fromParams(route.params);
  const page = Math.max(0, Number(route.params.page ?? "0") || 0);
  const [draft, setDraft] = useState(applied);
  useEffect(() => setDraft(fromParams(route.params)), [route.params]);

  const filter: EventFilter = { ...applied, limit: PAGE, offset: page * PAGE };
  const state = useAsync((s) => getEvents(filter, s), [JSON.stringify(filter)]);

  const go = (values: Record<Field, string>, p: number) => {
    const params: Record<string, string | undefined> = { ...values, page: p ? String(p) : undefined };
    window.location.hash = hashFor("/events", params);
  };
  const set = (f: Field) => (e: { target: { value: string } }) =>
    setDraft((d) => ({ ...d, [f]: e.target.value }));

  return (
    <>
      <form
        className="filters"
        onSubmit={(e) => {
          e.preventDefault();
          go(draft, 0);
        }}
      >
        <label>
          Event type
          <select value={draft.rule} onChange={set("rule")}>
            <option value="">All</option>
            {RULES.map((r) => (
              <option key={r.family} value={r.family}>
                {r.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Ticker
          <input value={draft.ticker} onChange={set("ticker")} size={8} placeholder="TSLA" />
        </label>
        <label>
          Words
          <input value={draft.q} onChange={set("q")} placeholder="company or person" />
        </label>
        <label>
          From
          <input type="date" value={draft.since} onChange={set("since")} />
        </label>
        <label>
          To
          <input type="date" value={draft.until} onChange={set("until")} />
        </label>
        <label>
          Min. dollars
          <input
            value={draft.min_amount}
            onChange={set("min_amount")}
            size={10}
            inputMode="numeric"
            placeholder="1000000"
          />
        </label>
        <button className="primary" type="submit">
          Search
        </button>
        <button
          type="button"
          onClick={() => go({ rule: "", ticker: "", cik: "", q: "", since: "", until: "", min_amount: "" }, 0)}
        >
          Clear
        </button>
      </form>
      {applied.cik && (
        <div className="notice">
          Only company CIK {applied.cik} ·{" "}
          <button type="button" onClick={() => go({ ...applied, cik: "" }, 0)}>
            Show all companies
          </button>
        </div>
      )}

      <Loaded state={state}>
        {(r) => (
          <div className="card">
            <div className="sub">
              {count(r.total)} event{r.total === 1 ? "" : "s"}
              {r.built_at ? ` · feed built ${r.built_at.replace("T", " ").replace("Z", " UTC")}` : ""}
            </div>
            <EventList events={r.events} />
            <div className="pager">
              <button type="button" disabled={page === 0} onClick={() => go(applied, page - 1)}>
                Newer
              </button>
              <span className="muted">
                {r.total === 0 ? "0" : `${page * PAGE + 1}–${Math.min(r.total, page * PAGE + r.events.length)}`} of{" "}
                {count(r.total)}
              </span>
              <button
                type="button"
                disabled={(page + 1) * PAGE >= r.total}
                onClick={() => go(applied, page + 1)}
              >
                Older
              </button>
            </div>
          </div>
        )}
      </Loaded>
    </>
  );
}
