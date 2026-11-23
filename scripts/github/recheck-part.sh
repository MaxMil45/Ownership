#!/usr/bin/env bash
# One part of .github/workflows/recheck-periodic.yml: checks every 10-Q/10-K
# filed from <from> to <to> and keeps an excerpt of each report that shows a
# Rule 10b5-1 plan adopted or terminated. Output in out/: periodic/ (excerpts,
# sidecars, checked-part-<n>.tsv), the log and a one-line summary.
# A day that keeps failing is listed in the summary; the part still ends
# successfully so the next part runs, and the import reports the failed days.
set -u
part=$1
from=$2
to=$3

if [ -z "${OE_USER_AGENT:-}" ]; then
  echo "::error::The repository secret OE_USER_AGENT is not set (Settings > Secrets and variables > Actions): your name and email, as the SEC requires."
  exit 1
fi

mkdir -p out/periodic
log="out/part-$part.log"
./target/release/oe daily-periodic "$from" --to "$to" --all --out out/periodic >"$log" 2>&1
code=$?

if [ -f out/periodic/checked.tsv ]; then
  mv out/periodic/checked.tsv "out/periodic/checked-part-$part.tsv"
else
  touch "out/periodic/checked-part-$part.tsv"
fi

{
  echo "part $part, $from to $to: oe exit $code"
  grep -E '^all days:|^done:|day\(s\) failed' "$log" | tail -2
} >"out/summary-$part.txt"
cat "out/summary-$part.txt"
echo "--- end of the log"
tail -n 5 "$log"
exit 0
