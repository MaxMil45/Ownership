// Right-click "Look up" on selected text, and `oe <name>` in the address bar.
// Both open the extension's results page; nothing reads the pages you visit.

import { cleanQuery } from "./text.js";

const MENU = "oe-lookup";

function openResults(raw: string): void {
  const q = cleanQuery(raw);
  if (!q) return;
  void chrome.tabs.create({ url: chrome.runtime.getURL(`results.html?q=${encodeURIComponent(q)}`) });
}

chrome.runtime.onInstalled.addListener(() => {
  void chrome.contextMenus.removeAll().then(() =>
    chrome.contextMenus.create({ id: MENU, title: "Look up “%s” in Ownership Engine", contexts: ["selection"] }),
  );
});

chrome.contextMenus.onClicked.addListener((info) => {
  if (info.menuItemId === MENU && info.selectionText) openResults(info.selectionText);
});

chrome.omnibox.setDefaultSuggestion({ description: "Look up a person, fund, company or ticker in Ownership Engine" });
chrome.omnibox.onInputEntered.addListener(openResults);
