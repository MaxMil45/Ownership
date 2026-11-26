import { useEffect, useState } from "react";
import { navigate } from "./router";

/** One search for everything: people, funds, companies, tickers, members. */
export function SearchBox({ initial = "", big = false }: { initial?: string; big?: boolean }) {
  const [q, setQ] = useState(initial);
  useEffect(() => setQ(initial), [initial]);
  return (
    <form
      role="search"
      className={big ? "search big" : "search"}
      onSubmit={(e) => {
        e.preventDefault();
        const v = q.trim();
        if (v) navigate("/search", { q: v });
      }}
    >
      <input
        type="search"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder={big ? "A person, fund, company or ticker: Elon Musk, NVDA, Nancy Pelosi…" : "Search people, companies, tickers"}
        aria-label="Search people, funds, companies and tickers"
        autoFocus={big}
        maxLength={200}
      />
      <button className="primary" type="submit">
        Search
      </button>
    </form>
  );
}
