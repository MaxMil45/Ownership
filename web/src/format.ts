// Small formatting helpers. Text from filings is always rendered through
// React (escaped); links only through `safeUrl`.

/** `$1,234`, `$56.8K`, `$1.25M`, `$3.10B` (as the Rust side writes them). */
export function money(v: number): string {
  const a = Math.abs(v);
  const sign = v < 0 ? "-" : "";
  if (a >= 1e9) return `${sign}$${(a / 1e9).toFixed(2)}B`;
  if (a >= 1e6) return `${sign}$${(a / 1e6).toFixed(2)}M`;
  if (a >= 1e4) return `${sign}$${(a / 1e3).toFixed(1)}K`;
  return `${sign}$${Math.round(a).toLocaleString("en-US")}`;
}

/** `5.612004818614034` -> `5.6%`; small values keep two decimals. */
export function percent(p: number): string {
  const digits = Math.abs(p) >= 10 ? 1 : Math.abs(p) >= 0.1 ? 2 : 3;
  return `${p.toFixed(digits)}%`;
}

export function count(n: number): string {
  return n.toLocaleString("en-US");
}

/** Only links to the SEC and the House Clerk's disclosure site are made. */
export function safeUrl(url: string): string | null {
  try {
    const u = new URL(url);
    const ok =
      u.protocol === "https:" &&
      (u.hostname === "www.sec.gov" || u.hostname === "disclosures-clerk.house.gov");
    return ok ? u.toString() : null;
  } catch {
    return null;
  }
}

/** `insider.cluster_buy@2` -> `insider.cluster_buy`. */
export function ruleFamily(ruleId: string): string {
  return ruleId.split("@")[0] ?? ruleId;
}

export const RULES: { family: string; label: string; color: string }[] = [
  { family: "insider.cluster_buy", label: "Cluster buy", color: "var(--s1)" },
  { family: "insider.planned_sale", label: "Planned sale (Form 144)", color: "var(--s2)" },
  { family: "insider.plan_terminated", label: "10b5-1 plan ended", color: "var(--s3)" },
  { family: "activist.new_13d", label: "New 13D", color: "var(--s4)" },
  { family: "activist.passive_to_active", label: "Passive to active", color: "var(--s5)" },
  { family: "activist.proxy_contest", label: "Proxy contest", color: "var(--s6)" },
];

export function ruleLabel(ruleId: string): string {
  const f = ruleFamily(ruleId);
  return RULES.find((r) => r.family === f)?.label ?? f;
}

export function ruleColor(ruleId: string): string {
  const f = ruleFamily(ruleId);
  return RULES.find((r) => r.family === f)?.color ?? "var(--axis)";
}

/** `2026-11-23` -> `2026-11`. */
export const monthOf = (day: string): string => day.slice(0, 7);

export function today(): string {
  return new Date().toISOString().slice(0, 10);
}

/** `n` days before `day` (`YYYY-MM-DD`), as `YYYY-MM-DD`. */
export function daysBefore(day: string, n: number): string {
  const d = new Date(`${day}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() - n);
  return d.toISOString().slice(0, 10);
}
