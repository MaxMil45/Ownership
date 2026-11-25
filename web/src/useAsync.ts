import { useEffect, useState } from "react";

export interface Async<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
}

/** Runs `load` when `deps` change; an older answer never replaces a newer one. */
export function useAsync<T>(load: (signal: AbortSignal) => Promise<T>, deps: unknown[]): Async<T> {
  const [state, setState] = useState<Async<T>>({ data: null, error: null, loading: true });
  useEffect(() => {
    const ctl = new AbortController();
    setState((s) => ({ data: s.data, error: null, loading: true }));
    load(ctl.signal).then(
      (data) => {
        if (!ctl.signal.aborted) setState({ data, error: null, loading: false });
      },
      (e: unknown) => {
        if (ctl.signal.aborted) return;
        setState({ data: null, error: e instanceof Error ? e.message : String(e), loading: false });
      },
    );
    return () => ctl.abort();
    // `load` is recreated each render; the caller names what it depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return state;
}
