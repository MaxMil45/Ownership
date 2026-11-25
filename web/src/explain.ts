// What a kind of trade or event usually means, in plain words. General
// background, the same for everyone: never a claim about why one person
// traded (the filings do not say).

export const EXPLAIN: Record<string, { title: string; text: string }> = {
  buy: {
    title: "Open-market buy (code P)",
    text:
      "The insider bought shares at the market price with their own money. Insiders get shares in many ways " +
      "(pay, options, grants), so a purchase is a choice to add more. It is rarer than selling and is often " +
      "read as confidence in the company, though it promises nothing about the share price.",
  },
  sell: {
    title: "Open-market sale (code S)",
    text:
      "The insider sold shares at the market price. Insiders sell for many reasons that have nothing to do " +
      "with the company's prospects: taxes on shares they were paid with, buying a house, spreading their " +
      "money. Much of their wealth is in one stock, so steady selling is normal. Sudden or unusually large " +
      "selling stands out more, and selling by several insiders at once more still.",
  },
  plan: {
    title: "10b5-1 plan",
    text:
      "A schedule of trades the insider set up in advance, at a time they had no inside information; the " +
      "trades then happen automatically. Since 2023 a plan must wait at least 90 days before its first trade. " +
      "Trades under a plan say less about what the insider thinks today.",
  },
  cluster: {
    title: "Cluster buy",
    text:
      "Three or more insiders of the same company bought within ten days of each other. Several people with " +
      "the best view of a company deciding to buy at the same time is the pattern researchers have most often " +
      "found worth noticing. Routine yearly buyers and plan trades are left out.",
  },
  planned_sale: {
    title: "Planned sale (Form 144)",
    text:
      "Notice that an insider or large holder intends to sell restricted or control shares, filed on or before " +
      "the sale. It is an intention: some notices are never sold in full. \"Routine\" means the same seller has " +
      "filed one most months; \"first in a year\" is the less usual case.",
  },
  plan_ended: {
    title: "10b5-1 plan ended",
    text:
      "A company reported that an insider ended a pre-arranged trading plan. Ending a plan early, especially " +
      "before any trade under it, is unusual and can mean the insider's view changed.",
  },
  new_13d: {
    title: "New 13D",
    text:
      "Someone now owns more than 5% of a class of the company's shares and may want to influence it (change " +
      "the board, push for a sale). Item 4 of the filing says what they plan. Passive investors file the " +
      "shorter 13G instead.",
  },
  passive_to_active: {
    title: "Passive to active",
    text:
      "A holder who had filed as a passive investor (13G) now files a 13D: they no longer rule out trying to " +
      "influence the company.",
  },
  proxy_contest: {
    title: "Proxy contest",
    text:
      "Someone other than the board is asking shareholders to vote their way, usually for their own board " +
      "candidates. These are public fights over who runs the company.",
  },
  price_line: {
    title: "The price line",
    text:
      "The average price insiders paid or received each day they traded, as written in their filings. It is not " +
      "the market price: there are gaps when no insider traded, and prices are not adjusted for stock splits " +
      "(Tesla's 2020 and 2022 splits show as drops).",
  },
  filing_date: {
    title: "Filing date only",
    text:
      "Filings before October 2026 come from the SEC's quarterly data sets, which give the day a filing was made " +
      "but not the time. Newer ones carry the exact time the SEC accepted them.",
  },
};

/** The explanation for an event's rule, if there is one. */
export function explainRule(ruleId: string): { title: string; text: string } | undefined {
  const family = ruleId.split("@")[0];
  const key: Record<string, string> = {
    "insider.cluster_buy": "cluster",
    "insider.planned_sale": "planned_sale",
    "insider.plan_terminated": "plan_ended",
    "activist.new_13d": "new_13d",
    "activist.passive_to_active": "passive_to_active",
    "activist.proxy_contest": "proxy_contest",
  };
  const k = family ? key[family] : undefined;
  return k ? EXPLAIN[k] : undefined;
}
