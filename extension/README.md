# Ownership Engine browser extension

Looks up people, funds, companies and tickers in your own Ownership Engine:
`oe serve` running on this computer. Chrome and Edge (Manifest V3). First
version: lookups only.

## Install (once)

1. Build it: `.\scripts\start.ps1` does it, or by hand:

   ```powershell
   cd extension; npm ci; npm run build
   ```

2. Open `chrome://extensions` (Edge: `edge://extensions`), turn on
   **Developer mode**, click **Load unpacked** and pick `extension\dist`.
3. Right-click the toolbar icon, **Options**, and paste the token from
   `data\serve\token.txt` in the repository folder. **Check and save**
   tests it against `oe serve`.

After a rebuild, click the reload arrow on the extension's card.

## Use

- **Toolbar popup:** type a name or ticker.
- **Right-click:** select a name on any page, then "Look up ... in
  Ownership Engine". Opens a results tab.
- **Address bar:** type `oe`, a space, then a name or ticker, and press Enter.

Results list companies (with their latest events), people and funds (who
they are in the filings, their latest trade) and members of Congress (their
latest trades), each linked to the source filing. **Open profile**, **Open
company** and **Search in the app** open the web app at that page, already
signed in for the current run of `oe serve` (`GET /v1/app-link`).

## What it can see and send

- No content scripts: it does not read the pages you visit. The right-click
  menu gets only the text you selected, when you choose it.
- Permissions: `storage` (the token and address), `contextMenus`, and
  `http://127.0.0.1/*` and `http://localhost/*` (to reach `oe serve`).
- The token is kept in the browser's local extension storage (never synced)
  and sent only to 127.0.0.1 or localhost; the options refuse any other
  address. Text from filings is put on the page as text, never as HTML, and
  only sec.gov and House Clerk links are made.

## Develop

```powershell
npm test           # the pure helpers in src/text.ts
npm run typecheck
npm run build      # tsc into dist/, then build.mjs copies static/ and draws the icons
```

`src/chrome.d.ts` declares the few extension APIs used, by hand, so the build
needs only TypeScript and Vitest. Not yet: Firefox, and marking names or
tickers on the pages you read (that would need content scripts and a broader
permission, a decision to make first).
