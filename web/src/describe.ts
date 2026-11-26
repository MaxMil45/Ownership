// Plain sentences about a lookup candidate: who they are in the filings and
// what they did last. Facts from the filings only, never a reason.

import type { Candidate } from "./api";
import { count, money, percent } from "./format";

/** "director and CEO", "10% owner", "insider". */
export function roleWords(r: Candidate["roles"][number]): string {
  const parts: string[] = [];
  if (r.officer) parts.push(r.titles[0] ? r.titles[0] : "officer");
  if (r.director) parts.push("director");
  if (r.ten_percent_owner) parts.push("10% owner");
  if (r.other && parts.length === 0) parts.push("insider");
  return parts.length ? joinWords(parts) : "insider";
}

export function joinWords(w: string[]): string {
  if (w.length <= 1) return w[0] ?? "";
  return `${w.slice(0, -1).join(", ")} and ${w[w.length - 1]}`;
}

/** What kind of filer this is, as a short label for a badge. */
export function kindLabel(kind: string): string {
  switch (kind) {
    case "insider":
      return "Company insider";
    case "holder":
      return "5%+ holder";
    case "contest":
      return "Proxy fight";
    case "official":
      return "Member of Congress";
    default:
      return kind;
  }
}

function years(first: string | null, last: string | null): string {
  const a = first?.slice(0, 4);
  const b = last?.slice(0, 4);
  if (!a || !b) return "";
  return a === b ? `in ${a}` : `from ${a} to ${b}`;
}

/** One line for a search result: who they are, in the fewest words. */
export function oneLine(c: Candidate): string {
  if (c.official) {
    const term = c.official.terms[0] ?? "";
    return term ? `Member of Congress (${term})` : "Member of Congress";
  }
  if (c.roles.length > 0) {
    const top = [...c.roles].sort((a, b) => b.last_on.localeCompare(a.last_on));
    const first = top[0]!;
    const more = top.length > 1 ? ` and ${top.length - 1} other compan${top.length === 2 ? "y" : "ies"}` : "";
    return `${roleWords(first)} at ${first.issuer_name}${more}`;
  }
  if (c.holdings.length > 0) {
    const h = c.holdings[0]!;
    return `Holds over 5% of ${h.issuer_name}${c.holdings.length > 1 ? " and others" : ""}`;
  }
  if (c.contests.length > 0) return `Filed contested proxy material about ${c.contests[0]!.subject_name}`;
  return `${count(c.filings)} filings`;
}

/** The "In short" paragraph at the top of a profile. */
export function inShort(c: Candidate): string[] {
  const out: string[] = [];
  const span = years(c.first_on, c.last_on);
  // A member's "filings" are their terms and reports; they get their own line below.
  if (!c.official || c.roles.length > 0 || c.holdings.length > 0) {
    out.push(
      `${c.display_name} appears in ${count(c.filings)} filing${c.filings === 1 ? "" : "s"}${span ? ` ${span}` : ""}.`,
    );
  }
  if (c.roles.length > 0) {
    const byRecent = [...c.roles].sort((a, b) => b.last_on.localeCompare(a.last_on));
    const shown = byRecent.slice(0, 3).map((r) => `${roleWords(r)} at ${r.issuer_name}${r.ticker ? ` (${r.ticker})` : ""}`);
    const rest = byRecent.length - shown.length;
    out.push(
      `Reported as ${joinWords(shown)}${rest > 0 ? `, and at ${rest} more compan${rest === 1 ? "y" : "ies"}` : ""}.`,
    );
  }
  const t = c.trades[0];
  if (t) {
    const what = t.code === "P" ? "bought" : "sold";
    const shares = t.shares !== null ? `${count(t.shares)} shares of ` : "shares of ";
    const value = t.value !== null ? ` for about ${money(t.value)}` : "";
    out.push(`Latest open-market trade: ${what} ${shares}${t.ticker ?? t.issuer_name}${value} on ${t.transaction_date}.`);
  } else if (c.roles.length > 0) {
    out.push("No open-market buys or sales on file (grants, option exercises and gifts are not counted).");
  }
  const h = c.holdings[0];
  if (h) {
    out.push(
      `Latest 5%+ stake: ${h.submission_type} on ${h.issuer_name}${h.percent !== null ? `, ${percent(h.percent)}` : ""}, filed ${h.receipt.known_on}.`,
    );
  }
  if (c.official) {
    const term = c.official.terms[0];
    out.push(
      `Member of Congress${term ? ` (latest term: ${term})` : ""}, with ${count(c.official.reports)} trade report${c.official.reports === 1 ? "" : "s"} on file` +
        (c.official.last_filed ? `, the latest filed ${c.official.last_filed}.` : "."),
    );
  }
  if (!c.exact) out.push("This is a partial match for the name you typed: check it is the right person.");
  return out;
}
