import { useEffect } from "react";
import { EXPLAIN } from "../explain";
import { GLOSSARY, GROUPS } from "../glossary";
import type { Route } from "../router";

const STEPS: { title: string; text: string }[] = [
  {
    title: "1. Search",
    text: "Type a name or ticker in the box at the top. Companies, members of Congress and people or funds in SEC filings are listed apart. Two people with the same name are never merged: check the company and years.",
  },
  {
    title: "2. Open a profile",
    text: "A person's page starts with \"In short\": who they are in the filings and what they did last. Below are their roles, their buying and selling pattern at each company, and the filings themselves.",
  },
  {
    title: "3. Look at the chart",
    text: "On a company page, ▲ is an insider's buy and ▼ a sale, sized by value; ◆ is a member of Congress's trade. Pick people in the list beside it to see only their trades.",
  },
  {
    title: "4. Check the source",
    text: "Every trade and event links to the filing it comes from. The app reads filings; it does not judge them.",
  },
];

/** How to use the app, and every word it uses. */
export function Guide({ route }: { route: Route }) {
  const term = route.params.term;
  // Jump to the asked-for word (the address's hash is the app's route).
  useEffect(() => {
    if (!term) return;
    const el = document.getElementById(`term-${term}`);
    el?.scrollIntoView({ block: "center" });
    el?.classList.add("flash");
    const t = setTimeout(() => el?.classList.remove("flash"), 1600);
    return () => clearTimeout(t);
  }, [term]);

  return (
    <>
      <div className="grid4">
        {STEPS.map((s) => (
          <div className="card" key={s.title}>
            <h2>{s.title}</h2>
            <p className="sub">{s.text}</p>
          </div>
        ))}
      </div>

      <div className="card">
        <h2>What this can and cannot tell you</h2>
        <ul className="plain">
          <li>
            <strong>It shows what was filed, and when it became public.</strong> Filings say what someone did, not
            why. A sale can be for taxes or a house; a buy promises nothing about the price.
          </li>
          <li>
            <strong>It is not advice.</strong> Nothing here says what to buy or sell. The History page describes
            the past only.
          </li>
          <li>
            <strong>It can be incomplete.</strong> Filers make typos (impossible amounts are left out of totals),
            some filings are late, and Senate reports and scanned House reports are not read yet.
          </li>
          <li>
            <strong>It stays on this computer.</strong> The data is in your own database; the app talks only to{" "}
            <code>oe serve</code> on 127.0.0.1.
          </li>
        </ul>
      </div>

      {GROUPS.map((g) => (
        <div className="card" key={g.id}>
          <h2>{g.title}</h2>
          <dl className="glossary">
            {GLOSSARY.filter((e) => e.group === g.id).map((e) => (
              <div key={e.id} id={`term-${e.id}`} className={term === e.id ? "picked" : undefined}>
                <dt>{e.term}</dt>
                <dd>
                  <strong>{e.short}</strong> {e.long}
                </dd>
              </div>
            ))}
          </dl>
        </div>
      ))}

      <div className="card">
        <h2>The kinds of trades and events</h2>
        <dl className="glossary">
          {Object.entries(EXPLAIN).map(([k, e]) => (
            <div key={k}>
              <dt>{e.title}</dt>
              <dd>{e.text}</dd>
            </div>
          ))}
        </dl>
      </div>
    </>
  );
}
