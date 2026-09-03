# Contributing

This project trades a funded account. Contributions are welcome, and the bar is higher than
usual for exactly that reason.

## Before you start

Read [`docs/ENGINEERING-STANDARDS.md`](docs/ENGINEERING-STANDARDS.md). CI enforces most of
it; a PR that fails those gates will not be reviewed until it is green.

For anything non-trivial, open an issue first. For anything architectural, expect to write an
ADR — see [`docs/DECISIONS.md`](docs/DECISIONS.md).

## Contributor Licence Agreement

**By submitting a pull request you agree to the following.**

You certify that:

1. The contribution is your original work, or you have the right to submit it under this
   agreement.
2. You grant the repository owner a perpetual, worldwide, irrevocable, royalty-free licence
   to use, reproduce, modify, distribute, sublicense, and **relicense** your contribution,
   including under licence terms different from the project's current licence.
3. You retain copyright in your contribution.
4. Your contribution contains no code carried under GPL, AGPL, LGPL, Elastic Licence,
   Business Source Licence, CC-BY-SA, or any non-commercial or field-of-use restriction, and
   no code copied from a project without a permissive licence.

Point 2 exists so the project's licence can change later without tracking down every past
contributor. Point 4 exists because a single copyleft line forecloses that option entirely.
See [`docs/LICENSING.md`](docs/LICENSING.md).

If you cannot agree to these terms, please open an issue describing the change instead of
submitting code.

## Clean-room rule

If your change was informed by another project, and that project is **not** MIT / Apache-2.0
/ BSD / ISC / Zlib:

1. Do not have its source open while writing.
2. Work from a written description of the behaviour.
3. Add an entry to [`docs/PRIOR-ART.md`](docs/PRIOR-ART.md) naming the project, its licence,
   what concept you took, and stating that no code was used.

## Workflow

```bash
git checkout -b feat/short-description
# ... changes ...
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
```

- Branch from `main`. No direct pushes to `main`.
- **Conventional commits:** `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`.
- One logical change per PR. Split refactors from behaviour changes.
- New dependencies: state what it does, why the standard library will not do, and its licence.

## Review checklist

Every PR is checked against this. Reviewers will ask about any unticked box.

- [ ] Does any new order path bypass `RiskGate`? **If yes, the PR is rejected.**
- [ ] Are new errors classified as `Transient` / `Fatal` / `Rejected` / `Invariant`?
- [ ] Is money `Decimal` throughout — no floats?
- [ ] Any `Utc::now()` or unseeded randomness in strategy or gate code?
- [ ] Are secrets absent from logs, error messages, and API responses?
- [ ] Does every new dependency carry a permitted licence?
- [ ] Is there a regression test that fails without the fix?
- [ ] Does an irreversible decision need an ADR?
- [ ] Is `docs/` still accurate after this change?

## Security issues

**Do not open a public issue.** Use GitHub private vulnerability reporting — see
[`docs/SECURITY.md`](docs/SECURITY.md#reporting-a-vulnerability).

## What is unlikely to be merged

- Anything that lets an order reach a venue without passing the risk gate.
- Anything that stores a credential in the database, the config file, or a log.
- A control that fails *open* on error.
- Live-venue code without a paper-mode equivalent.
- A strategy that cannot be exercised deterministically in a test.
