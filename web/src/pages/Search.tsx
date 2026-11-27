import { findCompanies, getEvents, lookup, type Candidate } from "../api";
import { kindLabel, oneLine } from "../describe";
import { count, ruleLabel } from "../format";
import { hashFor, type Route } from "../router";
import { SearchBox } from "../SearchBox";
import { Term } from "../ui";
import { useAsync } from "../useAsync";

/** Looks like a ticker: 1 to 6 letters, maybe a dot class (BRK.B), maybe a $. */
export function asTicker(q: string): string | null {
  const m = /^\$?([A-Za-z]{1,6}(?:[.-][A-Za-z]{1,2})?)$/.exec(q.trim());
  return m ? m[1]!.toUpperCase() : null;
}

function PersonRow({ c, q }: { c: Candidate; q: string }) {
  return (
    <li className="result">
      <a className="result-name" href={hashFor("/lookup", { name: q, key: c.key })}>
        {c.display_name}
      </a>
      <span className="result-line">{oneLine(c)}</span>
      <span className="result-meta">
        {c.kinds.map((k) => (
          <span className={`tag kind-${k}`} key={k}>
            {kindLabel(k)}
          </span>
        ))}
        {count(c.filings)} filing{c.filings === 1 ? "" : "s"}
        {c.first_on && c.last_on ? `, ${c.first_on.slice(0, 4)}–${c.last_on.slice(0, 4)}` : ""}
        {!c.exact && " · partial match"}
      </span>
    </li>
  );
}

function CompanyRow({ cik, name, ticker }: { cik: number; name: string; ticker: string | null }) {
  // The two latest events, to show at a glance what happened there.
  const ev = useAsync((s) => getEvents({ cik: String(cik), limit: 2 }, s), [cik]);
  return (
    <li className="result">
      <a className="result-name" href={hashFor("/company", { cik: String(cik) })}>
        {name}
        {ticker ? ` (${ticker})` : ""}
      </a>
      <span className="result-line">
        {ev.data
          ? ev.data.total === 0
            ? "No events found by the rules."
            : `${count(ev.data.total)} event${ev.data.total === 1 ? "" : "s"}; latest: ${ruleLabel(ev.data.events[0]!.rule_id)} on ${ev.data.events[0]!.known_on ?? "?"}`
          : "…"}
      </span>
      <span className="result-meta">
        <span className="tag">Company</span>
        <Term id="cik">CIK</Term> {cik}
      </span>
    </li>
  );
}

export function Search({ route }: { route: Route }) {
  const q = (route.params.q ?? "").trim();
  const ticker = asTicker(q);
  const companies = useAsync((s) => (q ? findCompanies(q, s) : Promise.resolve(null)), [q]);
  const people = useAsync((s) => (q ? lookup(q, s) : Promise.resolve(null)), [q]);

  const list = companies.data?.companies ?? [];
  // An exact ticker goes first.
  const sorted = ticker ? [...list].sort((a, b) => Number(b.ticker === ticker) - Number(a.ticker === ticker)) : list;
  const found = people.data?.result === "found" ? people.data.candidates : [];
  const members = found.filter((c) => c.official);
  const others = found.filter((c) => !c.official);
  const nothing = companies.data && people.data && sorted.length === 0 && found.length === 0;

  return (
    <>
      <SearchBox initial={q} big />
      {!q && (
        <div className="notice">
          Type a name or ticker. People with the same name are shown apart, each with their own filings.
        </div>
      )}
      {q && (
        <div className="results">
          {(companies.error || people.error) && <div className="error">{companies.error ?? people.error}</div>}
          {nothing && (
            <div className="notice">
              Nothing found for “{q}”. Try the last name only, the company's name as filed (for example “Apple
              Inc”), or its ticker. Having no match says nothing about a person: many people never file.
            </div>
          )}
          {people.data && (people.data.result !== "found" || (found.length > 0 && people.data.message)) && (
            <div className="notice">{people.data.message}</div>
          )}

          {sorted.length > 0 && (
            <section>
              <h2>Companies</h2>
              <ul className="result-list">
                {sorted.slice(0, 8).map((c) => (
                  <CompanyRow key={c.cik} {...c} />
                ))}
              </ul>
            </section>
          )}
          {members.length > 0 && (
            <section>
              <h2>Members of Congress</h2>
              <ul className="result-list">
                {members.map((c) => (
                  <PersonRow key={c.key} c={c} q={q} />
                ))}
              </ul>
            </section>
          )}
          {others.length > 0 && (
            <section>
              <h2>People and funds in SEC filings</h2>
              <ul className="result-list">
                {others.map((c) => (
                  <PersonRow key={c.key} c={c} q={q} />
                ))}
              </ul>
            </section>
          )}
          {(companies.loading || people.loading) && <div className="muted">Searching…</div>}
        </div>
      )}
    </>
  );
}
