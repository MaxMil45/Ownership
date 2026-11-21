# Release checklist (v0.1.0, Gate 1)

Prepared 2026-11-21. Publishing is done by the maintainer, not by tools.

## Done in the repository

- [x] `LICENSE` is the official Apache-2.0 text, byte for byte (11,358 bytes, SHA-256 `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30`, downloaded from https://www.apache.org/licenses/LICENSE-2.0.txt and checked against the copy shown in a browser). The copyright line is in `NOTICE`.
- [x] `Cargo.toml`: `repository = "https://github.com/MaxMil45/Ownership"`; `rust-version = "1.88"`, the real minimum (the dependencies need it; it said 1.75).
- [x] CI runs every project check: format, clippy (with and without the optional features), tests on Linux and Windows, the build without network support, the 13 PostgreSQL tests against a `postgres:16` service, and a check with exactly Rust 1.88.
- [x] `CHANGELOG.md` describes 0.1.0, with its known limits.
- [x] Real fixtures hold no home addresses: every Form 4 fixture's owner address is a company office or a PO box; the Form 144, 13D/G and proxy fixtures were chosen to name no individual's address.
- [x] `data/`, `.env` and the database stay out of git (`.gitignore`).

## Before tagging

- [ ] Push and see all CI jobs green on `main`, including the two new ones (`PostgreSQL tests`, `minimum Rust version`), which have not run yet.
- [ ] Gate 1 asks for alerts within 5 minutes of filing. Measured so far: a Form 4 86 s and a Form 144 62 s after acceptance (two filings, 2026-11-20). Run `oe follow --alert` through one full trading day (Monday 2026-11-23 or later) and record the delays.
- [ ] Optional: try one real Discord webhook (`OE_DISCORD_WEBHOOK_URL`).

## Releasing

1. In `Cargo.toml`, change `version = "0.1.0-dev"` to `version = "0.1.0"`; run `cargo check` so `Cargo.lock` follows.
2. In `CHANGELOG.md`, replace "(unreleased)" with the date.
3. Commit, then tag and push:

   ```powershell
   git tag -a v0.1.0 -m "v0.1.0"
   git push origin main --tags
   ```

4. On GitHub, create a release from the tag with the 0.1.0 section of the changelog as its notes.
5. Afterwards, set the version to `0.2.0-dev`.

## Not part of this release

- Publishing data: the data licence (ADR 0003) is undecided.
- Email alerts; database storage of Form 144, 13D/G, proxy and Item 408 events.
