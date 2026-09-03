---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Grade target — B (70+)

The repository is presented for licensing against a published rubric. **B (70+) licenses
reliably; below B goes to a case-by-case pass.** This is the target.

The grader **reads the actual code**. Padded docs and aspirational READMEs are flagged, not
rewarded. So the plan below does not add documentation to chase a score — it names the real
work, and where in the roadmap it already lives.

## Honest starting point

As of 2026-09-03 the repository is a **C (~55–60)**: clean layering, real tests in CI, and an
honest doc set, sitting on a thin codebase with a few hygiene gaps. The gap to B is mostly
**code substance and process**, not docs.

## Rubric dimensions → where the gap closes

| Dimension | What B needs | Closed by |
|---|---|---|
| Architecture & Robustness | Clear layering, real error handling, validated input | Layering already good. Error taxonomy → S3/S7. Input validation → **S2.1**. Retry / circuit breaker → **S5**, **S7.6** |
| Test Coverage | Real tests on real behaviour, in CI | Real tests exist. Coverage reporting → **Pre-S0 (H7)**. Property tests on the gate → **S5.4**. Surface grows with S1–S5 |
| Docs & Onboarding | A README a new dev can run from | **Already at B/A.** Do not add more — it reads as padding |
| Security Hygiene | No committed secrets, enforced authorization | `gitleaks` gate → **Pre-S0 (H3)**. AuthZ → **S9** (RBAC) |
| Code Cleanliness | Consistent style, no dead code, linted in CI | `fmt` + `clippy` already gated. `forbid(unsafe_code)` + workspace lints → **Pre-S0 (H5)**. Unwired `sniper` / `copytrade` — labelled deferred in [ARCHITECTURE.md](ARCHITECTURE.md); wired or foldered by v0.2 |
| History & Maintenance | Conventional commits, releases, active repo | Conventional commits from now on. `CHANGELOG.md` + `v0.0.1` tag → **Pre-S0 (H6)** |
| Dependency Health | Lockfile committed, lean deps, auto-updates | Commit `Cargo.lock` → **Pre-S0 (H1)**. Renovate → **Pre-S0 (H2)** |
| CI/CD Maturity | Lint, type-check, tests on every PR | Present. `cargo-deny` + `cargo-audit` + branch protection + PR template → **Pre-S0 (H3, H4)** |
| **Secrets & Threat Modeling** (security repo type) | Secrets manager, `SECURITY.md`, threat model | `SECURITY.md` and `THREAT-MODEL.md` written (threat model finalised at S15). Secrets manager → **S6** |

## Pre-S0 hygiene checklist (H-items)

Cheap, no architecture, all already implied by [ROADMAP.md](ROADMAP.md#pre-s0--repo-bootstrap).
Pulled out here because they are what lifts the process half of the rubric immediately.

- [x] **H1** — `Cargo.lock` un-ignored and committed
- [x] **H2** — `renovate.json`
- [x] **H3** — CI gates: `cargo-deny` (+ `deny.toml`; covers RustSec advisories), `gitleaks`, SBOM, doc-links
- [x] **H4** — `PULL_REQUEST_TEMPLATE.md`, `CODEOWNERS`, issue templates, **branch protection on `main`** (enabled once the repo went public — strict required checks, PR required, linear history)
- [x] **H5** — `[workspace.lints]` with `unsafe_code = "forbid"` and the clippy denies, inherited by every crate; `missing_docs` / `pedantic` phased in per crate
- [x] **H6** — `CHANGELOG.md` *(tag `v0.0.1` cut after this PR merges)*
- [x] **H7** — `cargo-llvm-cov` summary step in CI
- [x] **H8** — `CLAUDE.md`
- [x] **H9** — `AppConfig::validate` (defect 10, 6 tests) and clean Ctrl-C shutdown (defect 9, 2 tests)

## The honest conclusion

H1–H8 are documentation and configuration; they can be done now and get five dimensions to
B-level. **Architecture and Test Coverage do not reach B until S1–S5 are actually built** —
the grader reads code, and a scaffold with good docs is still a scaffold. This document
records the target; it does not shortcut the build.
