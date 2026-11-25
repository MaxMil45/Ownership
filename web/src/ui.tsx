import type { ReactNode } from "react";
import type { Async } from "./useAsync";
import { safeUrl } from "./format";

/** A link to an SEC or House Clerk page; anything else is shown as text. */
export function ExternalLink({ url, children }: { url: string; children: ReactNode }) {
  const href = safeUrl(url);
  if (!href) return <>{children}</>;
  return (
    <a href={href} target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  );
}

/** Shows the error or a loading note; renders `children` once there is data. */
export function Loaded<T>({
  state,
  children,
}: {
  state: Async<T>;
  children: (data: T) => ReactNode;
}) {
  if (state.error) return <div className="error">{state.error}</div>;
  if (!state.data) return <div className="muted">Loading…</div>;
  return <>{children(state.data)}</>;
}
