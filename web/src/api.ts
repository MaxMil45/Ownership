// Typed client for the `oe serve` API (docs/how-to-run.md, "The local API").
//
// Two ways to reach it:
// * served: the built app comes from `oe serve` itself (scripts/start.ps1).
//   Requests go to the same address and carry the HttpOnly cookie the login
//   link set; no token is ever seen by this code.
// * development (`npm run dev`): the API is another origin; the token and
//   address live in this browser's localStorage (Settings page).

/** True for the built app that `oe serve` hosts. */
export const SERVED = !import.meta.env.DEV;

export interface Settings {
  baseUrl: string;
  token: string;
}

export const DEFAULT_BASE_URL = "http://127.0.0.1:8730";
const KEY = "oe-settings";

export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const s = JSON.parse(raw) as Partial<Settings>;
      return {
        baseUrl: typeof s.baseUrl === "string" && s.baseUrl ? s.baseUrl : DEFAULT_BASE_URL,
        token: typeof s.token === "string" ? s.token : "",
      };
    }
  } catch {
    // fall through to the defaults
  }
  return { baseUrl: DEFAULT_BASE_URL, token: "" };
}

export function saveSettings(s: Settings): void {
  localStorage.setItem(KEY, JSON.stringify(s));
  window.dispatchEvent(new Event("oe-settings"));
}

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

// ---- response types (field names are the API's) --------------------------

export interface EventItem {
  key: string;
  rule_id: string;
  known_on: string | null;
  issuer_cik: number | null;
  issuer_name: string | null;
  ticker: string | null;
  headline: string;
  details: string[];
  people: { cik: number | null; name: string }[];
  receipts: { accession: string; form: string | null; url: string }[];
  text: string;
  amount_usd: number | null;
  percent: number | null;
}

export interface EventsResponse {
  total: number;
  limit: number;
  offset: number;
  built_at: string | null;
  events: EventItem[];
}

export interface CountsResponse {
  counts: { day: string; rule_id: string; events: number }[];
}

export interface FlowResponse {
  days: { day: string; buy_value: number; buys: number; sell_value: number; sells: number }[];
  implausible_rows_left_out: number;
}

export interface StatusResponse {
  filings: number;
  bulk_filings: number;
  transactions: number;
  transactions_known_now: number;
  form144_notices: number;
  schedule13_filings: number;
  contest_filings: number;
  trading_arrangements: number;
  earliest_accepted: string | null;
  latest_accepted: string | null;
  migrations: string[];
}

export interface Receipt {
  accession: string;
  url: string;
  known_on: string;
  filing_date_only: boolean;
}

export interface Candidate {
  key: string;
  display_name: string;
  kinds: string[];
  other_names: string[];
  filings: number;
  first_on: string | null;
  last_on: string | null;
  exact: boolean;
  roles: {
    issuer_cik: number;
    issuer_name: string;
    ticker: string | null;
    director: boolean;
    officer: boolean;
    ten_percent_owner: boolean;
    other: boolean;
    titles: string[];
    first_on: string;
    last_on: string;
    filings: number;
  }[];
  trades: {
    transaction_date: string;
    code: string;
    shares: number | null;
    price: number | null;
    value: number | null;
    issuer_name: string;
    ticker: string | null;
    receipt: Receipt;
  }[];
  holdings: {
    issuer_cik: number;
    issuer_name: string;
    class_title: string;
    submission_type: string;
    percent: number | null;
    receipt: Receipt;
  }[];
  contests: { subject_name: string; form: string; receipt: Receipt }[];
  official: {
    bioguide: string;
    name: string;
    terms: string[];
    committees: { name: string; title: string | null; as_of: string }[];
    reports: number;
    scanned_reports: number;
    first_filed: string | null;
    last_filed: string | null;
    recent_reports: { filed_on: string; doc_id: string; scanned: boolean; url: string }[];
    use_limits: string;
  } | null;
}

export interface LookupResponse {
  result: "found" | "needs_a_name" | "too_broad";
  count?: number;
  message: string;
  candidates: Candidate[];
}

export interface CompanyResponse {
  cik: number;
  name: string;
  ticker: string | null;
  filings: number;
  first_on: string | null;
  last_on: string | null;
  insiders: {
    owner_cik: number;
    name: string;
    director: boolean;
    officer: boolean;
    ten_percent_owner: boolean;
    other: boolean;
    titles: string[];
    first_on: string;
    last_on: string;
    filings: number;
  }[];
  trades: {
    transaction_date: string;
    code: string;
    owner_name: string;
    shares: number | null;
    price: number | null;
    value: number | null;
    receipt: Receipt;
  }[];
  flow: FlowResponse;
}

export interface CompaniesResponse {
  companies: { cik: number; name: string; ticker: string | null }[];
}

export interface EventFilter {
  since?: string;
  until?: string;
  rule?: string;
  ticker?: string;
  cik?: string;
  min_amount?: string;
  q?: string;
  limit?: number;
  offset?: number;
}

// ---- requests -------------------------------------------------------------

/** The query string of `GET /v1/events`: only the filters that are set. */
export function eventsQuery(f: EventFilter): string {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(f)) {
    if (v === undefined || v === null) continue;
    const text = String(v).trim();
    if (text !== "") p.set(k, text);
  }
  const q = p.toString();
  return q ? `?${q}` : "";
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  let url: string;
  let headers: Record<string, string> = { ...((init.headers as Record<string, string>) ?? {}) };
  if (SERVED) {
    url = "/v1" + path;
  } else {
    const { baseUrl, token } = loadSettings();
    if (!token) {
      throw new ApiError(0, "No token yet: open Settings and paste the one `oe serve` prints.");
    }
    url = baseUrl.replace(/\/+$/, "") + "/v1" + path;
    headers = { Authorization: `Bearer ${token}`, ...headers };
  }
  let res: Response;
  try {
    res = await fetch(url, { ...init, headers, credentials: "same-origin" });
  } catch (e) {
    if (e instanceof DOMException && e.name === "AbortError") throw e;
    throw new ApiError(
      0,
      SERVED
        ? "Cannot reach oe serve. Is its window still open? Run scripts\\start.ps1 again."
        : "Cannot reach the API. Is `oe serve --allow-origin http://127.0.0.1:5173` running?",
    );
  }
  let body: unknown = null;
  try {
    body = await res.json();
  } catch {
    // not JSON
  }
  if (res.status === 401 && SERVED) {
    throw new ApiError(
      401,
      "This browser is not signed in. Open the link the oe serve window printed (or run scripts\\start.ps1 again).",
    );
  }
  if (!res.ok) {
    const msg =
      body && typeof body === "object" && "error" in body
        ? String((body as { error: unknown }).error)
        : res.statusText;
    throw new ApiError(res.status, msg);
  }
  return body as T;
}

export const getEvents = (f: EventFilter, signal?: AbortSignal) =>
  request<EventsResponse>(`/events${eventsQuery(f)}`, { signal });
export const getCounts = (signal?: AbortSignal) =>
  request<CountsResponse>("/events/counts", { signal });
export const getFlow = (signal?: AbortSignal) => request<FlowResponse>("/flow", { signal });
export const getStatus = (signal?: AbortSignal) => request<StatusResponse>("/status", { signal });
export const getCompany = (cik: number, signal?: AbortSignal) =>
  request<CompanyResponse>(`/company?cik=${encodeURIComponent(String(cik))}`, { signal });
export const findCompanies = (q: string, signal?: AbortSignal) =>
  request<CompaniesResponse>(`/companies?q=${encodeURIComponent(q)}`, { signal });
export const lookup = (name: string, signal?: AbortSignal) =>
  request<LookupResponse>("/lookup", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name }),
    signal,
  });
