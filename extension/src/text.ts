// Pure helpers: no browser APIs, so they are tested on their own.

export const DEFAULT_BASE_URL = "http://127.0.0.1:8730";
export const MAX_QUERY = 200;

/**
 * Text someone selected on a page, made into a search: one line, no
 * surrounding quotes or punctuation, no honorifics, at most 200 characters.
 * `null` if nothing useful is left.
 */
export function cleanQuery(raw: string): string | null {
  let q = raw.replace(/\s+/g, " ").trim();
  q = q.replace(/^[\s"'“”‘’(\[]+|[\s"'“”‘’)\].,;:!?]+$/g, "");
  q = q.replace(/^(mr|mrs|ms|dr|sen|rep|hon)\.?\s+/i, "");
  q = q.replace(/[’']s$/, "");
  if (q.length > MAX_QUERY) q = q.slice(0, MAX_QUERY).replace(/\s+\S*$/, "");
  return q.length >= 2 ? q : null;
}

/** `$TSLA`, `nvda`, `BRK.B` -> the ticker; a name -> null. */
export function asTicker(q: string): string | null {
  const m = /^\$?([A-Za-z]{1,6}(?:[.-][A-Za-z]{1,2})?)$/.exec(q.trim());
  return m ? m[1]!.toUpperCase() : null;
}

/**
 * The address of oe serve, only ever on this computer: the token must never
 * go anywhere else. Returns the normalized address or an error message.
 */
export function checkBaseUrl(raw: string): { ok: string } | { error: string } {
  let u: URL;
  try {
    u = new URL(raw.trim());
  } catch {
    return { error: "Not an address. Use http://127.0.0.1:8730" };
  }
  if (u.protocol !== "http:") return { error: "Use http:// (oe serve runs on this computer)." };
  if (u.hostname !== "127.0.0.1" && u.hostname !== "localhost") {
    return { error: "Only 127.0.0.1 or localhost: the token must not leave this computer." };
  }
  if (u.username || u.password || (u.pathname !== "/" && u.pathname !== "") || u.search || u.hash) {
    return { error: "Only the address and port, for example http://127.0.0.1:8730" };
  }
  return { ok: `http://${u.hostname}:${u.port || "80"}` };
}

/** A 64-character hex token, as `oe serve` writes to data/serve/token.txt. */
export function checkToken(raw: string): string | null {
  const t = raw.trim();
  return /^[0-9a-f]{64}$/i.test(t) ? t : null;
}

/** Only links to the SEC and the House Clerk's disclosure site are made. */
export function safeUrl(url: string): string | null {
  try {
    const u = new URL(url);
    const ok =
      u.protocol === "https:" && (u.hostname === "www.sec.gov" || u.hostname === "disclosures-clerk.house.gov");
    return ok ? u.toString() : null;
  } catch {
    return null;
  }
}

export function money(v: number): string {
  const a = Math.abs(v);
  const sign = v < 0 ? "-" : "";
  if (a >= 1e9) return `${sign}$${(a / 1e9).toFixed(2)}B`;
  if (a >= 1e6) return `${sign}$${(a / 1e6).toFixed(2)}M`;
  if (a >= 1e4) return `${sign}$${(a / 1e3).toFixed(1)}K`;
  return `${sign}$${Math.round(a).toLocaleString("en-US")}`;
}

export const count = (n: number): string => n.toLocaleString("en-US");

// ---- what the API answers (only the fields used here) ---------------------

export interface Role {
  issuer_cik: number;
  issuer_name: string;
  ticker: string | null;
  director: boolean;
  officer: boolean;
  ten_percent_owner: boolean;
  other: boolean;
  titles: string[];
  last_on: string;
}

export interface Candidate {
  key: string;
  display_name: string;
  kinds: string[];
  filings: number;
  first_on: string | null;
  last_on: string | null;
  exact: boolean;
  roles: Role[];
  trades: {
    transaction_date: string;
    code: string;
    shares: number | null;
    value: number | null;
    issuer_name: string;
    ticker: string | null;
    receipt: { url: string };
  }[];
  holdings: { issuer_name: string; submission_type: string; percent: number | null }[];
  contests: { subject_name: string }[];
  official: { bioguide: string; terms: string[]; reports: number; last_filed: string | null } | null;
}

export interface OfficialTrade {
  owner: string | null;
  asset: string;
  ticker: string | null;
  tx_type: string;
  tx_date: string | null;
  amount: string;
  amount_min: number | null;
  amount_max: number | null;
  report_url: string;
}

export interface LookupResponse {
  result: "found" | "needs_a_name" | "too_broad";
  message: string;
  candidates: Candidate[];
}

export interface Company {
  cik: number;
  name: string;
  ticker: string | null;
}

export interface EventItem {
  rule_id: string;
  known_on: string | null;
  headline: string;
  receipts: { url: string }[];
}

// ---- plain words ----------------------------------------------------------

export function roleWords(r: Role): string {
  const parts: string[] = [];
  if (r.officer) parts.push(r.titles[0] ? r.titles[0] : "officer");
  if (r.director) parts.push("director");
  if (r.ten_percent_owner) parts.push("10% owner");
  return parts.length ? parts.join(", ") : "insider";
}

export function kindLabel(kind: string): string {
  return (
    { insider: "Insider", holder: "5%+ holder", contest: "Proxy fight", official: "Congress" }[kind] ?? kind
  );
}

/** Who they are, in one line. */
export function whoLine(c: Candidate): string {
  if (c.official) return c.official.terms[0] ? `Member of Congress (${c.official.terms[0]})` : "Member of Congress";
  const roles = [...c.roles].sort((a, b) => b.last_on.localeCompare(a.last_on));
  const r = roles[0];
  if (r) {
    const more = roles.length > 1 ? ` +${roles.length - 1} more` : "";
    return `${roleWords(r)} at ${r.issuer_name}${more}`;
  }
  const h = c.holdings[0];
  if (h) return `Holds over 5% of ${h.issuer_name}`;
  const k = c.contests[0];
  if (k) return `Proxy fight at ${k.subject_name}`;
  return `${count(c.filings)} filings`;
}

/** What they did last, if anything. */
export function lastLine(c: Candidate): string | null {
  const t = c.trades[0];
  if (t) {
    const what = t.code === "P" ? "Bought" : "Sold";
    const value = t.value !== null ? ` (${money(t.value)})` : "";
    return `${what} ${t.ticker ?? t.issuer_name}${value} on ${t.transaction_date}`;
  }
  if (c.official && c.official.reports > 0) {
    return `${count(c.official.reports)} trade reports, latest filed ${c.official.last_filed ?? "?"}`;
  }
  const h = c.holdings[0];
  if (h) return `${h.submission_type}${h.percent !== null ? `, ${h.percent.toFixed(1)}%` : ""}`;
  return null;
}

/** `insider.cluster_buy@1` -> `Cluster buy`. */
export function ruleLabel(ruleId: string): string {
  const f = ruleId.split("@")[0] ?? ruleId;
  return (
    {
      "insider.cluster_buy": "Cluster buy",
      "insider.planned_sale": "Planned sale",
      "insider.plan_terminated": "Plan ended",
      "activist.new_13d": "New 13D",
      "activist.passive_to_active": "Passive to active",
      "activist.proxy_contest": "Proxy fight",
    }[f] ?? f
  );
}

/** The app's address for a search or a profile, after its login link. */
export function appPath(login: string, to: { q: string } | { name: string; key: string } | { cik: number }): string {
  const p = new URLSearchParams();
  let route: string;
  if ("q" in to) {
    route = "/search";
    p.set("q", to.q);
  } else if ("cik" in to) {
    route = "/company";
    p.set("cik", String(to.cik));
  } else {
    route = "/lookup";
    p.set("name", to.name);
    p.set("key", to.key);
  }
  return `${login}#${route}?${p.toString()}`;
}

/** A member's trade in a few words: "Spouse bought NVDA, $1.00M–$5.00M, 2026-01-14". */
export function officialTradeLine(t: OfficialTrade): string {
  const who = { SP: "Spouse", JT: "Joint", DC: "Child" }[t.owner ?? ""] ?? "Self";
  const what = t.tx_type === "P" ? "bought" : t.tx_type === "E" ? "exchanged" : t.tx_type === "S (partial)" ? "sold part of" : "sold";
  const range =
    t.amount_min === null
      ? t.amount || "?"
      : t.amount_max === null
        ? `over ${money(t.amount_min - 1)}`
        : `${money(t.amount_min)}–${money(t.amount_max)}`;
  const asset = t.ticker ?? (t.asset.length > 40 ? `${t.asset.slice(0, 40)}…` : t.asset);
  return `${who} ${what} ${asset}, ${range}${t.tx_date ? `, ${t.tx_date}` : ""}`;
}
