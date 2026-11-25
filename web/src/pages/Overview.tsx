import { useState } from "react";
import { RangeSwitch } from "../RangeSwitch";
import { getCounts, getEvents, getFlow, getStatus } from "../api";
import { BuySellChart, EventsChart } from "../charts";
import { EventList } from "../EventList";
import { count, daysBefore, today } from "../format";
import { navigate } from "../router";
import { Loaded } from "../ui";
import { useAsync } from "../useAsync";

export function Overview() {
  const [years, setYears] = useState<number | null>(5);
  const status = useAsync((s) => getStatus(s), []);
  const flow = useAsync((s) => getFlow(s), []);
  const counts = useAsync((s) => getCounts(s), []);
  const latest = useAsync((s) => getEvents({ limit: 10 }, s), []);
  const recent = useAsync((s) => getEvents({ since: daysBefore(today(), 30), limit: 1 }, s), []);

  return (
    <>
      <Loaded state={status}>
        {(s) => (
          <div className="tiles">
            <div className="tile">
              <div className="label">Filings in the ledger</div>
              <div className="value">{count(s.filings)}</div>
              <div className="note">to {s.latest_accepted?.slice(0, 10) ?? "?"}</div>
            </div>
            <div className="tile">
              <div className="label">Insider trade rows</div>
              <div className="value">{count(s.transactions_known_now)}</div>
            </div>
            <div className="tile">
              <div className="label">Form 144 notices</div>
              <div className="value">{count(s.form144_notices)}</div>
            </div>
            <div className="tile">
              <div className="label">13D/G filings</div>
              <div className="value">{count(s.schedule13_filings)}</div>
            </div>
            <div className="tile">
              <div className="label">Events, last 30 days</div>
              <div className="value">{recent.data ? count(recent.data.total) : "…"}</div>
              <div className="note">{latest.data ? `${count(latest.data.total)} in all` : ""}</div>
            </div>
          </div>
        )}
      </Loaded>

      <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 8 }}>
        <RangeSwitch years={years} set={setYears} />
      </div>
      <div className="card">
        <h2>Events per month</h2>
        <div className="sub">Every rule, by the day the filing became public.</div>
        <Loaded state={counts}>{(c) => <EventsChart counts={c} years={years} />}</Loaded>
      </div>

      <div className="card">
        <h2>Insider buying and selling</h2>
        <div className="sub">Open-market purchases and sales reported on Forms 3/4/5, per month.</div>
        <Loaded state={flow}>
          {(f) => (
            <>
              <BuySellChart flow={f} years={years} />
              {f.implausible_rows_left_out > 0 && (
                <div className="muted">
                  {count(f.implausible_rows_left_out)} rows with impossible amounts as filed are left out.
                </div>
              )}
            </>
          )}
        </Loaded>
      </div>

      <div className="card">
        <h2>Latest events</h2>
        <Loaded state={latest}>{(l) => <EventList events={l.events} />}</Loaded>
        <p>
          <button type="button" onClick={() => navigate("/events")}>
            All events
          </button>
        </p>
      </div>
    </>
  );
}
