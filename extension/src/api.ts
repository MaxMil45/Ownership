// Talks to `oe serve` on this computer with the token the user pasted in
// the options page. The token is kept in this browser's extension storage
// (local, never synced) and only sent to 127.0.0.1 / localhost.

import {
  checkBaseUrl,
  DEFAULT_BASE_URL,
  type Company,
  type EventItem,
  type LookupResponse,
  type OfficialTrade,
} from "./text.js";

export interface Settings {
  baseUrl: string;
  token: string;
}

export async function loadSettings(): Promise<Settings> {
  const s = await chrome.storage.local.get(["baseUrl", "token"]);
  const base = typeof s.baseUrl === "string" ? checkBaseUrl(s.baseUrl) : null;
  return {
    baseUrl: base && "ok" in base ? base.ok : DEFAULT_BASE_URL,
    token: typeof s.token === "string" ? s.token : "",
  };
}

export async function saveSettings(s: Settings): Promise<void> {
  await chrome.storage.local.set({ baseUrl: s.baseUrl, token: s.token });
}

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init: RequestInit = {}, settings?: Settings): Promise<T> {
  const s = settings ?? (await loadSettings());
  if (!s.token) throw new ApiError(0, "No token yet. Open the extension's options and paste it.");
  const checked = checkBaseUrl(s.baseUrl);
  if ("error" in checked) throw new ApiError(0, checked.error);
  let res: Response;
  try {
    res = await fetch(`${checked.ok}/v1${path}`, {
      ...init,
      headers: { Authorization: `Bearer ${s.token}`, ...((init.headers as Record<string, string>) ?? {}) },
      credentials: "omit",
      cache: "no-store",
    });
  } catch {
    throw new ApiError(0, "Cannot reach oe serve. Start it with scripts\\start.ps1.");
  }
  let body: unknown = null;
  try {
    body = await res.json();
  } catch {
    // not JSON
  }
  if (!res.ok) {
    if (res.status === 401) throw new ApiError(401, "The token is wrong or old. Paste the current one in the options.");
    const msg = body && typeof body === "object" && "error" in body ? String(body.error) : res.statusText;
    throw new ApiError(res.status, msg);
  }
  return body as T;
}

export const lookup = (name: string) =>
  request<LookupResponse>("/lookup", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name }),
  });

export const findCompanies = (q: string) =>
  request<{ companies: Company[] }>(`/companies?q=${encodeURIComponent(q)}`);

export const companyEvents = (cik: number, limit: number) =>
  request<{ total: number; events: EventItem[] }>(`/events?cik=${cik}&limit=${limit}`);

export const memberTrades = (bioguide: string, limit: number) =>
  request<{ total: number; trades: OfficialTrade[] }>(
    `/officials/trades?bioguide=${encodeURIComponent(bioguide)}&limit=${limit}`,
  );

export const status = (settings?: Settings) =>
  request<{ filings: number; latest_accepted: string | null }>("/status", {}, settings);

/** The address that opens the web app signed in, for this run of oe serve. */
export async function appLogin(): Promise<string> {
  const s = await loadSettings();
  const r = await request<{ login: string }>("/app-link", {}, s);
  return `${s.baseUrl}${r.login}`;
}
