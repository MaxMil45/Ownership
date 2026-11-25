const RANGES: { label: string; years: number | null }[] = [
  { label: "1 year", years: 1 },
  { label: "5 years", years: 5 },
  { label: "All", years: null },
];

/** One year, five years or everything, for the charts. */
export function RangeSwitch({ years, set }: { years: number | null; set: (y: number | null) => void }) {
  return (
    <div className="seg" role="group" aria-label="Time range of the charts">
      {RANGES.map((r) => (
        <button key={r.label} type="button" aria-pressed={years === r.years} onClick={() => set(r.years)}>
          {r.label}
        </button>
      ))}
    </div>
  );
}
