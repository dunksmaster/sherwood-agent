<!-- Keep this short. The checklist is the point. -->

## What and why

<!-- One or two sentences. Link the issue or ADR if there is one. -->

## Checklist

<!-- From docs/ENGINEERING-STANDARDS.md and docs/DEFINITION-OF-DONE.md.
     Reviewers will ask about any unticked box. -->

- [ ] **No scaffold, no padding** — this adds behaviour a user or a test can observe, not
      infrastructure "for later" (no trait with a stub impl, no `pub` item with no caller, no
      parsed-but-unused config).
- [ ] No new order path bypasses `RiskGate`.
- [ ] New errors are classified `Transient` / `Fatal` / `Rejected` / `Invariant`.
- [ ] Money is `Decimal` — no floats.
- [ ] No `Utc::now()` or unseeded randomness in strategy or gate code.
- [ ] Secrets absent from logs, error messages, and API responses.
- [ ] Every new dependency carries a permitted licence (see `docs/LICENSING.md`).
- [ ] A regression test exists for any bug this fixes.
- [ ] An irreversible decision has an ADR.
- [ ] `docs/` (and `docs/CURRENT-STATE.md`) still accurate after this change.
- [ ] `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`, `cargo deny check` pass locally.
