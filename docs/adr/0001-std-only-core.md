# ADR 0001: Keep the core crates dependency-free

**Status:** accepted · **Date:** 2026-11-19

## Context

The core crates (`oe-core`, `oe-form4`, `oe-ledger`, `oe-events`) hold the logic that must be correct: parsing, dates, point-in-time rules, event definitions. They must build and test anywhere, including offline machines, CI runners and contributors' laptops, with fast compile times.

## Decision

- The core crates use only the Rust standard library.
- `oe-form4` has its own small XML tokenizer covering what SEC ownership XML uses: declaration, comments, CDATA, attributes, entity and character references.
- Network code lives in `oe-edgar` behind the `net` feature (on by default), so the rest builds without it.
- Dates use a small civil-date type in `oe-core` rather than `chrono`, because filings only carry calendar dates.

- **Update 2026-11-19:** `oe-edgar` also reads the SEC's JSON APIs (submissions, ticker list) with its own small JSON reader (`oe-edgar/src/json.rs`) instead of `serde_json`, for the same reason: it can be built and tested offline. The only external dependency in the workspace is `ureq`, behind the `net` feature.

## Consequences

- Faster builds and a tiny supply-chain surface for the parts that matter most.
- We maintain about 200 lines of tokenizer. If a real filing breaks it, add that filing as a fixture first, then fix the bug.
- The JSON reader loads a whole response into memory. Submissions files are a few megabytes, so that is fine; the nightly bulk `submissions.zip` will need streaming.
- **Revisit when:** we parse large XBRL documents (Item 408) or N-PORT files, where a streaming parser such as `quick-xml` is worth the dependency. Do that in a new crate, not by changing `oe-form4`.
