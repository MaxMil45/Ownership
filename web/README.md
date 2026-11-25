# Ownership Engine web app

A local app (TypeScript, React, Vite) that reads the `oe serve` API on this computer: an overview with charts, every event with filters and SEC receipts, and a name lookup. It sends nothing anywhere else, keeps its token in this browser's storage only, and links only to sec.gov and the House Clerk's disclosure site.

## Run it

From the repository folder:

```powershell
.\scripts\start.ps1
```

That builds this app, serves it from `oe serve` at <http://127.0.0.1:8730> and opens it signed in. Nothing else is needed.

## Work on it (live reloading)

In one terminal (from the repository folder, after `oe db-migrate` and `scripts\update.ps1` have filled the database):

```powershell
& $oe serve --allow-origin http://127.0.0.1:5173
```

In another:

```powershell
cd web
npm install      # once
npm run dev
```

Open <http://127.0.0.1:5173>, go to Settings, paste the token `oe serve` printed (also in `data\serve\token.txt`), and save.

## Commands

| Command | What it does |
| --- | --- |
| `npm run dev` | dev server on 127.0.0.1:5173 |
| `npm run build` | type check, then a production build in `dist/` |
| `npm run typecheck` | type check only |
| `npm test` | unit tests (formatting, links, filters, chart sums) |

## Layout

- `src/api.ts`: typed client for `/v1/events`, `/v1/events/counts`, `/v1/flow`, `/v1/status`, `/v1/lookup`, `/v1/company`, `/v1/companies`. Built (served by `oe serve`), it uses the same address and the login cookie; in development, the token pasted in Settings (kept in localStorage).
- `src/pages/`: Overview, Events, Companies, Lookup, Settings. Filters and the looked-up name live in the address (`#/events?ticker=TSLA`), so a view can be bookmarked.
- `src/charts.tsx`: the two charts, drawn as SVG without a charting library.
- `src/styles.css`: the dashboard's palette (light and dark by system setting).

Text from filings is shown through React (escaped); a link is made only when `safeUrl` accepts it.
