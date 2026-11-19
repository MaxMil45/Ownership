# Contributing

Thanks for helping. The most valuable contributions are often not code.

## Good first contributions

1. **A real filing that breaks the parser.** Add it to `fixtures/form4/real/`, describe the edge case in `fixtures/README.md`, and open an issue or a pull request with a failing test.
2. **A Field Guide correction.** If `docs/field-guide/` gets a rule wrong, fix it and cite the SEC source.
3. **Registry corrections** (from Phase 2): a wrong or missing link between filer identities, with the filing that proves it.

## Before opening a pull request

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Rules

- Every behaviour change to an event rule is a new version with its own spec in `events/specs/`.
- Every parser fix comes with a fixture that reproduces the bug.
- No private individuals. The project covers people only in public or market roles.
- No licensed data in the repository (CUSIP masters, price files, paid datasets).
- AI-assisted code is welcome; you are responsible for reviewing and understanding every line you submit.

By contributing you agree that your contributions are licensed under Apache-2.0.
