import { useEffect, useState, type ReactNode } from "react";
import { getStatus, loadSettings, SERVED } from "./api";
import { Company } from "./pages/Company";
import { Congress } from "./pages/Congress";
import { Events } from "./pages/Events";
import { Guide } from "./pages/Guide";
import { History } from "./pages/History";
import { Lookup } from "./pages/Lookup";
import { Overview } from "./pages/Overview";
import { Search } from "./pages/Search";
import { Settings } from "./pages/Settings";
import { useRoute } from "./router";
import { SearchBox } from "./SearchBox";
import { PageHead } from "./ui";
import { useAsync } from "./useAsync";

const NAV = [
  { path: "/", label: "Home" },
  { path: "/events", label: "Latest events" },
  { path: "/company", label: "Companies" },
  { path: "/congress", label: "Congress" },
  { path: "/history", label: "What happened after" },
  { path: "/guide", label: "Guide" },
];

/** Each page's title, what it shows, and the words to know for it. */
const HEADS: Record<string, { title: string; intro: ReactNode; terms?: string[] }> = {
  "/search": {
    title: "Search",
    intro: "Companies, members of Congress, and people or funds that appear in SEC filings, each listed apart.",
    terms: ["insider", "holder", "cik"],
  },
  "/lookup": {
    title: "Profile",
    intro: "Everything one filer has reported: roles, trades, stakes, and the filings behind them.",
    terms: ["insider", "open_market", "form4", "13d", "ptr"],
  },
  "/events": {
    title: "Latest events",
    intro:
      "Filings that matched one of the app's rules, newest first: cluster buys, planned sales, ended trading plans, new activist stakes, proxy fights. Filter by kind, ticker or date; click an event for what it means.",
    terms: ["event", "cluster", "form144", "plan", "13d", "proxy", "known_on"],
  },
  "/company": {
    title: "Companies",
    intro:
      "A company's insiders, their buys and sales on the price chart, members of Congress who traded it, and its events.",
    terms: ["insider", "open_market", "form4", "ptr"],
  },
  "/congress": {
    title: "Members of Congress",
    intro: "Stock trades House members reported for themselves, their spouse and children, newest first.",
    terms: ["ptr"],
  },
  "/history": {
    title: "What happened after",
    intro:
      "How stocks did after each kind of filing, from the first close after it became public, against the S&P 500. Compare each row with the yardstick, not with zero. History, not advice.",
    terms: ["vs_market", "yardstick", "known_on"],
  },
  "/guide": {
    title: "Guide",
    intro: "How to use the app, what it can and cannot tell you, and every word it uses.",
  },
  "/settings": { title: "Settings", intro: "Where the app finds oe serve." },
};

export function App() {
  const route = useRoute();
  const [token, setToken] = useState(() => loadSettings().token);
  useEffect(() => {
    const on = () => setToken(loadSettings().token);
    window.addEventListener("oe-settings", on);
    return () => window.removeEventListener("oe-settings", on);
  }, []);

  // In development, without a token only Settings works.
  const needsToken = !SERVED && !token;
  const path = needsToken ? "/settings" : route.path;
  const status = useAsync((s) => getStatus(s), [token]);
  const head = HEADS[path];
  const known = path === "/" || head !== undefined;
  useEffect(() => {
    document.title = head ? `${head.title} · Ownership Engine` : "Ownership Engine";
  }, [head]);
  return (
    <div className="app">
      <header className="topbar">
        <a className="brand" href="#/">
          Ownership Engine
        </a>
        <nav>
          {NAV.map((n) => (
            <a key={n.path} href={`#${n.path}`} aria-current={path === n.path ? "page" : undefined}>
              {n.label}
            </a>
          ))}
        </nav>
        {path !== "/" && path !== "/search" && <SearchBox />}
        <span className="right">
          <a
            href="#/settings"
            className="status"
            title="The data this app shows stays on this computer. Click for settings."
          >
            {status.data
              ? `● Data to ${status.data.latest_accepted?.slice(0, 10) ?? "?"}`
              : status.error
                ? "○ Not connected"
                : "Connecting…"}
          </a>
        </span>
      </header>
      {status.error && path !== "/settings" && <div className="error">{status.error}</div>}
      {needsToken && <div className="notice">Paste your token below to start.</div>}
      {head && <PageHead title={head.title} intro={head.intro} terms={head.terms} />}
      {path === "/" && <Overview />}
      {path === "/search" && <Search route={route} />}
      {path === "/events" && <Events route={route} />}
      {path === "/company" && <Company route={route} />}
      {path === "/congress" && <Congress route={route} />}
      {path === "/lookup" && <Lookup route={route} />}
      {path === "/history" && <History />}
      {path === "/guide" && <Guide route={route} />}
      {path === "/settings" && <Settings />}
      {!known && <div className="notice">No such page.</div>}
      <footer className="footer">
        From public SEC and House filings, read on this computer. Facts, not advice. <a href="#/guide">Guide</a>
      </footer>
    </div>
  );
}
