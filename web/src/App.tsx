import { useEffect, useState } from "react";
import { getStatus, loadSettings, SERVED } from "./api";
import { Company } from "./pages/Company";
import { Events } from "./pages/Events";
import { Lookup } from "./pages/Lookup";
import { Overview } from "./pages/Overview";
import { Settings } from "./pages/Settings";
import { useRoute } from "./router";
import { useAsync } from "./useAsync";

const NAV = [
  { path: "/", label: "Overview" },
  { path: "/events", label: "Events" },
  { path: "/company", label: "Companies" },
  { path: "/lookup", label: "Lookup" },
  { path: "/settings", label: "Settings" },
];

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
  return (
    <div className="app">
      <header className="topbar">
        <h1>Ownership Engine</h1>
        <nav>
          {NAV.map((n) => (
            <a
              key={n.path}
              href={`#${n.path}`}
              aria-current={path === n.path ? "page" : undefined}
            >
              {n.label}
            </a>
          ))}
        </nav>
        <span className="right" title="The data this app shows stays on this computer.">
          {status.data
            ? `● Connected · data to ${status.data.latest_accepted?.slice(0, 10) ?? "?"}`
            : status.error
              ? "○ Not connected"
              : "Connecting…"}
        </span>
      </header>
      {status.error && path !== "/settings" && <div className="error">{status.error}</div>}
      {needsToken && <div className="notice">Paste your token below to start.</div>}
      {path === "/" && <Overview />}
      {path === "/events" && <Events route={route} />}
      {path === "/company" && <Company route={route} />}
      {path === "/lookup" && <Lookup route={route} />}
      {path === "/settings" && <Settings />}
      {!NAV.some((n) => n.path === path) && <div className="notice">No such page.</div>}
    </div>
  );
}
