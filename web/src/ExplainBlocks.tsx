import { EXPLAIN } from "./explain";
import type { Insight } from "./insights";

/** "What does this mean?" for one topic of `EXPLAIN`, folded by default. */
export function Explain({ topic, title }: { topic: string; title?: string }) {
  const e = EXPLAIN[topic];
  if (!e) return null;
  return (
    <details className="explain">
      <summary>{title ?? `What is a ${e.title.toLowerCase()}?`}</summary>
      <p>{e.text}</p>
    </details>
  );
}

/** The summary sentences, each with a coloured dot. */
export function InsightList({ lines }: { lines: Insight[] }) {
  return (
    <ul className="insights">
      {lines.map((l, i) => (
        <li key={i}>
          <i className={`dot ${l.tone}`} aria-hidden="true" />
          {l.text}
        </li>
      ))}
    </ul>
  );
}

/** The four explanations every trade chart needs, side by side. */
export function TradeExplanations() {
  return (
    <div className="explain-row">
      <Explain topic="buy" title="What does a buy mean?" />
      <Explain topic="sell" title="What does a sale mean?" />
      <Explain topic="plan" title="What is a 10b5-1 plan?" />
      <Explain topic="price_line" title="Where do the prices come from?" />
    </div>
  );
}
