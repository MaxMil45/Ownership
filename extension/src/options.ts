// Where oe serve is and its token.

import { loadSettings, saveSettings, status } from "./api.js";
import { checkBaseUrl, checkToken, count } from "./text.js";

const base = document.getElementById("base") as HTMLInputElement;
const token = document.getElementById("token") as HTMLInputElement;
const msg = document.getElementById("msg") as HTMLElement;

function say(text: string, bad = false): void {
  msg.textContent = text;
  msg.className = bad ? "error" : "notice";
}

void loadSettings().then((s) => {
  base.value = s.baseUrl;
  token.value = s.token;
});

document.getElementById("form")?.addEventListener("submit", async (e) => {
  e.preventDefault();
  const b = checkBaseUrl(base.value);
  if ("error" in b) return say(b.error, true);
  const t = checkToken(token.value);
  if (!t) return say("The token is 64 letters and digits: the contents of data\serve\token.txt.", true);
  const settings = { baseUrl: b.ok, token: t };
  say("Checking…");
  try {
    const st = await status(settings);
    await saveSettings(settings);
    say(`Saved. Connected: ${count(st.filings)} filings, data to ${st.latest_accepted?.slice(0, 10) ?? "?"}.`);
  } catch (err) {
    say(`Not saved: ${err instanceof Error ? err.message : String(err)}`, true);
  }
});
