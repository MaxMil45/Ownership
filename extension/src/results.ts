// The results page, opened from the right-click menu or the address bar
// (`oe <name>`): results.html?q=...

import { search } from "./render.js";
import { cleanQuery } from "./text.js";

const form = document.getElementById("form") as HTMLFormElement;
const input = document.getElementById("q") as HTMLInputElement;
const out = document.getElementById("out") as HTMLElement;

function run(q: string): void {
  input.value = q;
  document.title = `${q} · Ownership Engine`;
  void search(q, out);
}

form.addEventListener("submit", (e) => {
  e.preventDefault();
  const q = cleanQuery(input.value);
  if (!q) return;
  history.replaceState(null, "", `?q=${encodeURIComponent(q)}`);
  run(q);
});

const start = cleanQuery(new URLSearchParams(location.search).get("q") ?? "");
if (start) run(start);
