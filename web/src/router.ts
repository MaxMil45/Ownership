import { useEffect, useState } from "react";

// A tiny hash router: `#/events?ticker=TSLA`. No dependency.

export interface Route {
  path: string;
  params: Record<string, string>;
}

export function parseHash(hash: string): Route {
  const raw = hash.replace(/^#/, "") || "/";
  const at = raw.indexOf("?");
  const path = at < 0 ? raw : raw.slice(0, at);
  const params: Record<string, string> = {};
  new URLSearchParams(at < 0 ? "" : raw.slice(at + 1)).forEach((v, k) => {
    params[k] = v;
  });
  return { path: path || "/", params };
}

export function hashFor(path: string, params: Record<string, string | undefined> = {}): string {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v) p.set(k, v);
  const q = p.toString();
  return `#${path}${q ? `?${q}` : ""}`;
}

export function navigate(path: string, params?: Record<string, string | undefined>): void {
  window.location.hash = hashFor(path, params);
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parseHash(window.location.hash));
  useEffect(() => {
    const on = () => setRoute(parseHash(window.location.hash));
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return route;
}
