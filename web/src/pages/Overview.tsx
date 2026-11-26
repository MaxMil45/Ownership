import { useState } from "react";
import { RangeSwitch } from "../RangeSwitch";
import { getCounts, getEvents, getFlow, getStatus } from "../api";
import { BuySellChart, EventsChart } from "../charts";
import { EventList } from "../EventList";
import { count, daysBefore, today } from "../format";
import { hashFor } from "../router";
import { SearchBox } from "../SearchBox";
import { Loaded, Term } from "../ui";
import { useAsync } from "../useAsync";

const EXAMPLES = ["Elon Musk", "NVDA", "Nancy Pelosi", "Elliott Investment Management", "Meta"];

/** What each part of the app answers, with a way in. */
const SECTIONS: { title: string; question: string; text: string; href: string; link: string }[] = [
  {
    title: "Insiders",
    question: "Are the people running a company buying or selling?",
    text: "Directors, officers and 10%+ owners report every trade in their company's shares within two business days (Form 4).",
    href: hashFor("/events", { rule: "insider.cluster_buy" }),
    link: "Latest cluster buys",
  },
  {
    title: "Planned sales",
    question: "Who has said they intend to sell?",
    text: "Insiders and large holders file a Form 144 before selling restricted shares. Routine sellers are marked as such.",
    href: hashFor("/events", { rule: "insider.planned_sale" }),
    link: "Latest planned sales",
  },
  {
    title: "Big holders",
    question: "Who owns more than 5%, and do they want changes?",
    text: "Funds file a 13D when they may push for change, a 13G when they stay passive; proxy fights are fights over the board.",
    href: hashFor("/events", { rule: "activist.new_13d" }),
    link: "Latest new 13Ds",
  },
  {
    title: "Congress",
    question: "What are members of the House trading?",
    text: "Members report trades by themselves, their spouse and children within 45 days, as value ranges.",
    href: "#/congress",
    link: "Members' trades",
  },
  {
    title: "What happened after",
    question: "How did stocks do after filings like these?",
    text: "Past results after each kind of filing, measured against the S&P 500 and an ordinary-month yardstick. History, not advice.",
    href: "#/history",
    link: "See the history",
  },
];

export function Overview() {
  const [years, setYears] = useState<number | null>(5);
  const status = useAsync((s) => getStatus(s), []);
  const flow = useAsync((s) => getFlow(s), []);
  const counts = useAsync((s) => getCounts(s), []);
  const latest = useAsync((s) => getEvents({ limit: 8 }, s), []);
  const recent = useAsync((s) => getEvents({ since: daysBefore(today(), 30), limit: 1 }, s), []);

  return (
    <>
      <section className="hero">
        <h1>Who owns, buys and sells what: from the public filings</h1>
        <p className="intro">
          Look up a person, a fund or a company to see what they have filed with the SEC (or, for members of
          Congress, with the House), what they bought and sold, and when it became public. Every fact links to
          its <Term id="receipt">source filing</Term>.
        </p>
        <SearchBox big />
        <p className="examples">
          Try:{" "}
          {EXAMPLES.map((e, i) => (
            <span key={e}>
              {i > 0 && " · "}
              <a href={hashFor("/search", { q: e })}>{e}</a>
            </span>
          ))}
        </p>
      </section>

      <section className="sections">
        {SECTIONS.map((s) => (
          <a key={s.title} className="section-card" href={s.href}>
            <span className="section-title">{s.title}</span>
            <span className="section-q">{s.question}</span>
            <span className="section-text">{s.text}</span>
            <span className="section-link">{s.link} →</span>
          </a>
        ))}
      </section>

      <div className="card">
        <h2>Latest events</h2>
        <div className="sub">
          Filings that matched one of the rules, newest first. Click one for what it means and the filings behind
          it. <a href="#/guide">New here? Read the guide.</a>
        </div>
        <Loaded state={latest}>{(l) => <EventList events={l.events} />}</Loaded>
        <p>
          <a href="#/events">All events, with filters →</a>
        </p>
      </div>

      <h2 className="section-heading">The big picture</h2>
      <Loaded state={status}>
        {(s) => (
          <div className="tiles">
            <div className="tile">
              <div className="label">Filings on this computer</div>
              <div className="value">{count(s.filings)}</div>
              <div className="note">
                {s.earliest_accepted?.slice(0, 4) ?? "?"} to {s.latest_accepted?.slice(0, 10) ?? "?"}
              </div>
            </div>
            <div className="tile">
              <div className="label">Insider trades</div>
              <div className="value">{count(s.transactions_known_now)}</div>
              <div className="note">rows on Forms 3, 4 and 5</div>
            </div>
            <div className="tile">
              <div className="label">Planned sales</div>
              <div className="value">{count(s.form144_notices)}</div>
              <div className="note">Form 144 notices</div>
            </div>
            <div className="tile">
              <div className="label">5%+ stakes</div>
              <div className="value">{count(s.schedule13_filings)}</div>
              <div className="note">13D and 13G filings</div>
            </div>
            <div className="tile">
              <div className="label">Events, last 30 days</div>
              <div className="value">{recent.data ? count(recent.data.total) : "…"}</div>
              <div className="note">{latest.data ? `${count(latest.data.total)} in all` : ""}</div>
            </div>
          </div>
        )}
      </Loaded>

      <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 8 }}>
        <RangeSwitch years={years} set={setYears} />
      </div>
      <div className="grid2">
        <div className="card">
          <h2>Insider buying and selling</h2>
          <div className="sub">
            Dollar value of <Term id="open_market">open-market</Term> buys and sales by insiders, per month.
            Selling is normally far larger than buying: insiders are paid in shares and sell some.
          </div>
          <Loaded state={flow}>
            {(f) => (
              <>
                <BuySellChart flow={f} years={years} />
                {f.implausible_rows_left_out > 0 && (
                  <div className="muted">
                    {count(f.implausible_rows_left_out)} rows with impossible amounts as filed are left out.
                  </div>
                )}
              </>
            )}
          </Loaded>
        </div>
        <div className="card">
          <h2>Events per month</h2>
          <div className="sub">How many filings matched each rule, by the day they became public.</div>
          <Loaded state={counts}>{(c) => <EventsChart counts={c} years={years} />}</Loaded>
        </div>
      </div>
    </>
  );
}
