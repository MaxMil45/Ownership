// Builds the results into the page with DOM calls and textContent only:
// text from filings is never parsed as HTML.

import { appLogin, companyEvents, findCompanies, lookup, memberTrades } from "./api.js";
import {
  appPath,
  asTicker,
  count,
  kindLabel,
  lastLine,
  officialTradeLine,
  ruleLabel,
  safeUrl,
  whoLine,
  type Candidate,
  type Company,
} from "./text.js";

type Child = Node | string | null | undefined | false;

export function h(tag: string, props: Record<string, string> = {}, ...children: Child[]): HTMLElement {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "class") el.className = v;
    else el.setAttribute(k, v);
  }
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(typeof c === "string" ? document.createTextNode(c) : c);
  }
  return el;
}

/** A link to a filing on sec.gov (or the House Clerk), else plain text. */
function sourceLink(url: string | undefined, text: string): Child {
  const href = url ? safeUrl(url) : null;
  return href ? h("a", { href, target: "_blank", rel: "noopener noreferrer" }, text) : null;
}

/** A button that opens the web app at `to`, signed in. */
function openButton(label: string, to: Parameters<typeof appPath>[1], onError: (m: string) => void): HTMLElement {
  const b = h("button", { type: "button", class: "open" }, label);
  b.addEventListener("click", async () => {
    try {
      const login = await appLogin();
      await chrome.tabs.create({ url: appPath(login, to) });
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  });
  return b;
}

/** A member of Congress's latest trades, filled in when they arrive. */
function memberTradeList(bioguide: string): HTMLElement {
  const list = h("ul", { class: "events" }, h("li", { class: "muted" }, "Loading their trades…"));
  memberTrades(bioguide, 3).then(
    (r) =>
      list.replaceChildren(
        ...(r.trades.length === 0
          ? [h("li", { class: "muted" }, "No trades read from their reports.")]
          : r.trades.map((t) => h("li", {}, officialTradeLine(t), " ", sourceLink(t.report_url, "report")))),
        ...(r.total > 3 ? [h("li", { class: "muted" }, `${count(r.total)} trades in all`)] : []),
      ),
    () => list.replaceChildren(h("li", { class: "muted" }, "Could not load their trades.")),
  );
  return list;
}

function personRow(c: Candidate, q: string, onError: (m: string) => void): HTMLElement {
  const last = lastLine(c);
  const trade = c.trades[0];
  return h(
    "li",
    { class: "row" },
    h("div", { class: "name" }, c.display_name),
    h("div", { class: "who" }, whoLine(c)),
    last ? h("div", { class: "last" }, last, " ", trade ? sourceLink(trade.receipt.url, "filing") : null) : null,
    c.official ? memberTradeList(c.official.bioguide) : null,
    h(
      "div",
      { class: "meta" },
      ...c.kinds.map((k) => h("span", { class: `tag kind-${k}` }, kindLabel(k))),
      `${count(c.filings)} filings`,
      c.first_on && c.last_on ? `, ${c.first_on.slice(0, 4)}–${c.last_on.slice(0, 4)}` : "",
      c.exact ? "" : " · partial match",
    ),
    openButton("Open profile", { name: q, key: c.key }, onError),
  );
}

function companyRow(c: Company, onError: (m: string) => void): HTMLElement {
  const events = h("ul", { class: "events" }, h("li", { class: "muted" }, "Loading events…"));
  companyEvents(c.cik, 3).then(
    (r) => {
      events.replaceChildren(
        ...(r.events.length === 0
          ? [h("li", { class: "muted" }, "No events found by the rules.")]
          : r.events.map((e) =>
              h(
                "li",
                {},
                h("span", { class: "rule" }, ruleLabel(e.rule_id)),
                ` ${e.known_on ?? ""} · ${e.headline} `,
                sourceLink(e.receipts[0]?.url, "filing"),
              ),
            )),
        ...(r.total > 3 ? [h("li", { class: "muted" }, `${count(r.total)} events in all`)] : []),
      );
    },
    () => events.replaceChildren(h("li", { class: "muted" }, "Could not load events.")),
  );
  return h(
    "li",
    { class: "row" },
    h("div", { class: "name" }, c.name, c.ticker ? ` (${c.ticker})` : ""),
    events,
    openButton("Open company", { cik: c.cik }, onError),
  );
}

/** Searches people and companies at once and fills `out`. */
export async function search(q: string, out: HTMLElement): Promise<void> {
  const errorBox = h("div", { class: "error", hidden: "" });
  const onError = (m: string) => {
    errorBox.textContent = m;
    errorBox.hidden = false;
  };
  out.replaceChildren(h("div", { class: "muted" }, `Searching for “${q}”…`));
  const [companies, people] = await Promise.allSettled([findCompanies(q), lookup(q)]);
  const parts: Child[] = [errorBox];

  const failed = [companies, people].find((r) => r.status === "rejected");
  if (failed && failed.status === "rejected") {
    out.replaceChildren(
      h("div", { class: "error" }, failed.reason instanceof Error ? failed.reason.message : String(failed.reason)),
    );
    return;
  }
  const ticker = asTicker(q);
  let list = companies.status === "fulfilled" ? companies.value.companies : [];
  if (ticker) list = [...list].sort((a, b) => Number(b.ticker === ticker) - Number(a.ticker === ticker));
  const found = people.status === "fulfilled" && people.value.result === "found" ? people.value.candidates : [];

  if (people.status === "fulfilled" && (people.value.result !== "found" || (found.length > 0 && people.value.message))) {
    parts.push(h("div", { class: "notice" }, people.value.message));
  }
  if (list.length > 0) {
    parts.push(h("h2", {}, "Companies"), h("ul", { class: "list" }, ...list.slice(0, 4).map((c) => companyRow(c, onError))));
  }
  if (found.length > 0) {
    parts.push(
      h("h2", {}, "People and funds"),
      h("ul", { class: "list" }, ...found.slice(0, 6).map((c) => personRow(c, q, onError))),
    );
    if (found.length > 6) parts.push(h("div", { class: "muted" }, `${found.length - 6} more in the app.`));
  }
  if (list.length === 0 && found.length === 0) {
    parts.push(
      h(
        "div",
        { class: "notice" },
        `Nothing found for “${q}”. Try the last name only, or the ticker. No match says nothing about a person: many people never file.`,
      ),
    );
  }
  parts.push(h("div", { class: "footer" }, openButton("Search in the app", { q }, onError)));
  out.replaceChildren(...parts.filter((p): p is Node | string => !!p));
}
