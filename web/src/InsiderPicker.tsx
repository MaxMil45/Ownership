import { useMemo, useState } from "react";
import { money } from "./format";
import type { Mark } from "./insights";

export interface InsiderTotals {
  owner_cik: number;
  name: string;
  bought: number;
  sold: number;
  trades: number;
  last: string;
}

/** Everyone who traded, with what they bought and sold, biggest first. */
export function insiderTotals(marks: Mark[]): InsiderTotals[] {
  const by = new Map<number, InsiderTotals>();
  for (const m of marks) {
    const e = by.get(m.owner_cik) ?? { owner_cik: m.owner_cik, name: m.owner_name, bought: 0, sold: 0, trades: 0, last: m.day };
    if (m.code === "P") e.bought += m.value;
    else e.sold += m.value;
    e.trades += m.trades;
    if (m.day >= e.last) {
      e.last = m.day;
      if (m.owner_name) e.name = m.owner_name;
    }
    by.set(m.owner_cik, e);
  }
  return [...by.values()].sort((a, b) => b.bought + b.sold - (a.bought + a.sold));
}

/** A checkbox per insider; `selected` null means everyone. */
export function InsiderPicker({
  marks,
  selected,
  onChange,
}: {
  marks: Mark[];
  selected: ReadonlySet<number> | null;
  onChange: (next: ReadonlySet<number> | null) => void;
}) {
  const people = useMemo(() => insiderTotals(marks), [marks]);
  const [filter, setFilter] = useState("");
  const shown = filter
    ? people.filter((p) => p.name.toLowerCase().includes(filter.trim().toLowerCase()))
    : people;
  const isOn = (cik: number) => selected === null || selected.has(cik);
  const toggle = (cik: number) => {
    const next = new Set(selected ?? people.map((p) => p.owner_cik));
    if (next.has(cik)) next.delete(cik);
    else next.add(cik);
    onChange(next.size === people.length ? null : next);
  };
  return (
    <div className="picker">
      <div className="picker-head">
        <strong>Whose trades to show</strong>
        <span className="muted">
          {selected === null ? `everyone (${people.length})` : `${selected.size} of ${people.length}`}
        </span>
        <button type="button" onClick={() => onChange(null)}>
          Everyone
        </button>
        <button type="button" onClick={() => onChange(new Set())}>
          Nobody
        </button>
        {people.length > 10 && (
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Find a name"
            size={16}
            aria-label="Find an insider"
          />
        )}
      </div>
      <ul className="picker-list">
        {shown.map((p) => (
          <li key={p.owner_cik}>
            <label>
              <input type="checkbox" checked={isOn(p.owner_cik)} onChange={() => toggle(p.owner_cik)} />
              <span className="name">{p.name || `CIK ${p.owner_cik}`}</span>
              <span className="muted">
                {p.bought > 0 && <span className="buy-text">bought {money(p.bought)}</span>}
                {p.bought > 0 && p.sold > 0 && " · "}
                {p.sold > 0 && <span className="sell-text">sold {money(p.sold)}</span>}
                {p.bought === 0 && p.sold === 0 && `${p.trades} trades without a price`}
                {` · last ${p.last.slice(0, 7)}`}
              </span>
            </label>
          </li>
        ))}
      </ul>
    </div>
  );
}
