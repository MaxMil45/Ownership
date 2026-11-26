// The words the app uses, in plain language. Shown on the Guide page and as
// hover notes wherever a <Term> appears. General background only.

export interface GlossaryEntry {
  id: string;
  term: string;
  /** One sentence, for the hover note. */
  short: string;
  /** A paragraph, for the Guide page. */
  long: string;
  group: "filings" | "people" | "events" | "reading";
}

export const GLOSSARY: GlossaryEntry[] = [
  // ---- the filings --------------------------------------------------------
  {
    id: "form4",
    term: "Form 4",
    group: "filings",
    short: "An insider's report of a trade in their own company's shares, due within two business days.",
    long:
      "Directors, officers and owners of more than 10% must tell the SEC within two business days each time " +
      "they buy, sell, are granted or exercise shares of their company. Form 3 is the first report when someone " +
      "becomes an insider; Form 5 is a yearly catch-up for small or late items. Most of what this app shows " +
      "about insiders comes from these.",
  },
  {
    id: "form144",
    term: "Form 144",
    group: "filings",
    short: "Notice that an insider or large holder plans to sell restricted or control shares.",
    long:
      "Filed on or before the sale when someone close to the company wants to sell shares that are restricted " +
      "(received from the company, not bought on the market) or that they control. It is an intention: some " +
      "notices are never sold in full. The sale itself then shows up on a Form 4.",
  },
  {
    id: "13d",
    term: "Schedule 13D",
    group: "filings",
    short: "Filed by anyone owning more than 5% of a company who may try to influence it.",
    long:
      "Due within 10 days of passing 5%. Item 4 says what the holder intends: to talk to the board, seek seats, " +
      "push for a sale, or nothing in particular. Activist investors file these.",
  },
  {
    id: "13g",
    term: "Schedule 13G",
    group: "filings",
    short: "The short form for a holder of more than 5% who stays passive.",
    long:
      "Index funds and other passive investors who own more than 5% file this shorter form. Switching from a " +
      "13G to a 13D means the holder no longer rules out trying to influence the company.",
  },
  {
    id: "proxy",
    term: "Contested proxy",
    group: "filings",
    short: "Material from someone other than the board asking shareholders to vote their way.",
    long:
      "Proxy statements ask shareholders how to vote. When someone other than the board files them (forms like " +
      "DFAN14A or PREC14A), there is a fight, usually over board seats.",
  },
  {
    id: "item408",
    term: "Item 408 (10-Q / 10-K)",
    group: "filings",
    short: "The part of a company's quarterly or yearly report that lists insiders' trading plans started or ended.",
    long:
      "Since 2023 companies must say each quarter which directors and officers adopted or ended a 10b5-1 trading " +
      "plan. This app reads the tagged data and flags plans that ended.",
  },
  {
    id: "ptr",
    term: "Periodic transaction report",
    group: "filings",
    short: "A House member's report of stock trades by themselves, their spouse or children, due within 45 days.",
    long:
      "The STOCK Act requires members of Congress to report trades over $1,000 within 45 days. Amounts are " +
      "ranges ($1,001 to $15,000, and so on), not exact figures. The app reads the typed reports filed with the " +
      "House Clerk since 2013; scanned paper reports are listed but not read. Senators file elsewhere and are not " +
      "included yet. Use of these reports is limited by law: not for commercial purposes (5 U.S.C. 13107).",
  },
  // ---- the people ---------------------------------------------------------
  {
    id: "insider",
    term: "Insider",
    group: "people",
    short: "A director, officer or owner of more than 10% of a company, who must report their trades.",
    long:
      "\"Insider\" is the SEC's word for the people who must file Forms 3, 4 and 5. It says nothing about " +
      "wrongdoing: insiders trade legally all the time, inside the rules.",
  },
  {
    id: "ten_percent",
    term: "10% owner",
    group: "people",
    short: "Someone (often a fund) who owns more than 10% of a class of the company's shares.",
    long: "They report trades like directors and officers do, on Form 4.",
  },
  {
    id: "holder",
    term: "Holder (5%+)",
    group: "people",
    short: "A person or fund that filed a 13D or 13G: they own more than 5% of the company.",
    long: "Large holders are usually funds. The percentage shown is the one they reported in the filing.",
  },
  {
    id: "cik",
    term: "CIK",
    group: "people",
    short: "The SEC's number for a company or person that files with it.",
    long:
      "Every filer gets a Central Index Key. Two people with the same name have different CIKs, so this app keeps " +
      "them apart and never merges them.",
  },
  // ---- the events ---------------------------------------------------------
  {
    id: "event",
    term: "Event",
    group: "events",
    short: "A filing, or a group of filings, that matches one of the app's rules (such as a cluster buy).",
    long:
      "Rules are written down in the project and versioned: when a rule changes it gets a new number (\"@2\"), " +
      "so old results can always be reproduced. Each event links to the SEC filings it comes from.",
  },
  {
    id: "cluster",
    term: "Cluster buy",
    group: "events",
    short: "Three or more insiders of one company bought on the open market within ten days of each other.",
    long:
      "Routine yearly buyers and trades under pre-arranged plans are left out. Several insiders choosing to buy " +
      "at the same time is the pattern researchers have most often found worth noticing.",
  },
  {
    id: "plan",
    term: "10b5-1 plan",
    group: "events",
    short: "A schedule of trades set up in advance, which then happen automatically.",
    long:
      "Insiders set these up at a time they hold no inside information. Trades under a plan say less about what " +
      "the insider thinks today; ending a plan early is less usual.",
  },
  // ---- reading the numbers ------------------------------------------------
  {
    id: "known_on",
    term: "Public on",
    group: "reading",
    short: "The day the SEC made the filing public (US Eastern time): the first day anyone could act on it.",
    long:
      "A trade happens on one day and is reported later. The app always uses the day it became public, so " +
      "nothing is measured with knowledge nobody had yet.",
  },
  {
    id: "open_market",
    term: "Open-market trade",
    group: "reading",
    short: "A buy (code P) or sale (code S) at the market price, as opposed to grants, gifts or option exercises.",
    long:
      "Insiders receive shares in many ways. Only open-market buys and sales are choices made with their own " +
      "money at the going price, so those are the ones the charts and summaries count.",
  },
  {
    id: "vs_market",
    term: "vs market",
    group: "reading",
    short: "The stock's change minus the S&P 500's (SPY) change over the same days.",
    long:
      "A stock that rose 10% while the market rose 12% did 2% worse than the market. Measured from the first " +
      "closing price after the filing became public, over 1, 3, 6 and 12 months of trading days.",
  },
  {
    id: "yardstick",
    term: "Yardstick",
    group: "reading",
    short: "Any company, starting on the first trading day of any month: what \"ordinary\" looks like.",
    long:
      "Most single stocks trail the index in most years, because a few very large winners lift it. So a typical " +
      "result is below zero even when nothing happened. Each kind of filing should be compared with the " +
      "yardstick, not with zero.",
  },
  {
    id: "receipt",
    term: "Receipt",
    group: "reading",
    short: "The link to the original filing on sec.gov (or the House Clerk's site) that a fact comes from.",
    long: "Everything shown can be checked against its source. If something looks wrong, open the receipt.",
  },
];

export const TERM: Record<string, GlossaryEntry> = Object.fromEntries(GLOSSARY.map((g) => [g.id, g]));

export const GROUPS: { id: GlossaryEntry["group"]; title: string }[] = [
  { id: "filings", title: "The filings" },
  { id: "people", title: "The people" },
  { id: "events", title: "The events" },
  { id: "reading", title: "Reading the numbers" },
];
