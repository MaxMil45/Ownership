import type { ReactNode } from "react";
import type { Async } from "./useAsync";
import { safeUrl } from "./format";
import { TERM } from "./glossary";
import { hashFor } from "./router";

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

/** A word from the glossary: dotted underline, hover note, link to the Guide. */
export function Term({ id, children }: { id: string; children?: ReactNode }) {
  const g = TERM[id];
  if (!g) return <>{children}</>;
  return (
    <a className="term" href={hashFor("/guide", { term: id })} title={g.short}>
      {children ?? g.term}
    </a>
  );
}

/** The title of a page, what it shows, and the words to know for it. */
export function PageHead({ title, intro, terms }: { title: string; intro: ReactNode; terms?: string[] }) {
  return (
    <div className="page-head">
      <h1>{title}</h1>
      <p className="intro">{intro}</p>
      {terms && terms.length > 0 && (
        <p className="terms">
          Words on this page:{" "}
          {terms.map((t, i) => (
            <span key={t}>
              {i > 0 && " · "}
              <Term id={t} />
            </span>
          ))}
        </p>
      )}
    </div>
  );
}
