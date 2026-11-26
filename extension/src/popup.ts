// The toolbar popup: one search box; results below.

import { loadSettings } from "./api.js";
import { search } from "./render.js";
import { cleanQuery } from "./text.js";

const form = document.getElementById("form") as HTMLFormElement;
const input = document.getElementById("q") as HTMLInputElement;
const out = document.getElementById("out") as HTMLElement;

document.getElementById("options")?.addEventListener("click", () => void chrome.runtime.openOptionsPage());

form.addEventListener("submit", (e) => {
  e.preventDefault();
  const q = cleanQuery(input.value);
  if (q) void search(q, out);
});

void loadSettings().then((s) => {
  if (!s.token) {
    out.textContent = "";
    const p = document.createElement("div");
    p.className = "notice";
    p.textContent = "First paste the token oe serve uses: click Options.";
    out.append(p);
  }
});
