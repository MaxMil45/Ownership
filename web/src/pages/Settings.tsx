import { useState } from "react";
import { DEFAULT_BASE_URL, loadSettings, saveSettings, SERVED } from "../api";

export function Settings() {
  const [s, setS] = useState(loadSettings);
  const [saved, setSaved] = useState(false);
  if (SERVED) {
    return (
      <div className="card" style={{ maxWidth: 640 }}>
        <h2>Connection</h2>
        <p className="sub">
          This app is served by <code>oe serve</code> on this computer, and this browser signed in through
          the link it printed. There is nothing to set. If pages say you are not signed in, run{" "}
          <code>.\scripts\start.ps1</code> again: it opens a fresh link.
        </p>
        <p className="sub">
          The browser extension uses the token in <code>dataserve	oken.txt</code>.
        </p>
      </div>
    );
  }
  return (
    <div className="card" style={{ maxWidth: 640 }}>
      <h2>Connection</h2>
      <p className="sub">
        Start the API in a terminal with{" "}
        <code>& $oe serve --allow-origin http://127.0.0.1:5173</code>. It prints a token, which is also in{" "}
        <code>dataserve	oken.txt</code>. Paste it here. It is kept in this browser only.
      </p>
      <form
        className="filters"
        style={{ flexDirection: "column", alignItems: "stretch" }}
        onSubmit={(e) => {
          e.preventDefault();
          saveSettings({ baseUrl: s.baseUrl.trim() || DEFAULT_BASE_URL, token: s.token.trim() });
          setSaved(true);
        }}
      >
        <label>
          API address
          <input value={s.baseUrl} onChange={(e) => setS({ ...s, baseUrl: e.target.value })} />
        </label>
        <label>
          Token
          <input
            type="password"
            autoComplete="off"
            value={s.token}
            onChange={(e) => setS({ ...s, token: e.target.value })}
          />
        </label>
        <div>
          <button className="primary" type="submit">
            Save
          </button>{" "}
          {saved && <span className="muted">Saved.</span>}
        </div>
      </form>
    </div>
  );
}
