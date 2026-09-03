---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Definition of done

The [grade target](GRADE-TARGET.md) is assessed by a model that reads the code. Padding and
aspirational structure are flagged, not rewarded. This document is how we avoid that: it
defines what "done" means so no step is checked off with stubs.

## Five rules

1. **Vertical slices, not horizontal scaffolding.** A step ships one capability end-to-end
   and wired in, proven by a test. Not "define the trait and six empty tables" — instead
   "kill the process, restart, the state is still there," with the test that proves it.

2. **No stub reaches `main`.** `todo!`, `unimplemented!`, `unreachable!` in non-test code are
   `deny` lints. An unfinished function stays on a branch.

3. **Every `pub` item has a caller or a behavioural test.** A public function with neither is
   dead code. Delete it; restore from history when it is actually needed.

4. **Docs never lead the code.** [CURRENT-STATE.md](CURRENT-STATE.md) is updated in the same
   PR as the change. Every capability a README or doc claims maps to a real code path or a
   test.

5. **Right-size the architecture.** A crate is added when a *second* consumer exists, not
   before. Four crates of real logic beat eleven crates of interfaces.

## Per-step exit criteria

A roadmap step (S1, S2, …) is not done until all of these hold:

- [ ] It compiles, and `cargo clippy -- -D warnings` is clean.
- [ ] Its tests assert **named real scenarios**, not that a setter set a field.
- [ ] The new behaviour is reachable — wired into the `sherwood` binary, or exercised by an
      integration test that runs it the way production would.
- [ ] No `todo!` / `unimplemented!` / stub returns in non-test code.
- [ ] `CURRENT-STATE.md` reflects the new reality — defects closed are struck, new gaps added.
- [ ] Coverage for the touched crate did not drop (or the PR explains why).
- [ ] Any doc claiming the new capability points at the code or test that delivers it.
- [ ] Conventional-commit history: each commit builds and tests green on its own.

## What counts as padding — and the fix

| Smell | Fix |
|---|---|
| A trait with one impl that returns a hardcoded value | Don't merge it until the impl does real work, or don't add the trait yet |
| A function no one calls | Delete it |
| Config fields parsed but never read | Wire them or remove them from the schema |
| Tests that only check construction | Rewrite as a behaviour test, or delete |
| A module skeleton full of `unimplemented!()` | Branch, not `main` |
| Comments describing intent where code should act | Write the code |
| "Enterprise" ceremony around a few hundred lines of logic | Collapse it; add structure when a real second consumer appears |
| Docs describing a feature that does not exist | Move the claim to `ROADMAP.md` as future work |

## The current known instance

`sherwood-sniper` and `sherwood-copytrade` are **real, tested library code** — seven concrete
rug-screen checks, three sizing modes with sell clamping, nine tests between them. They are
*not* stubs. Their only "smell" is that they are not wired into the runner, because they are
deferred to v0.2.

Each carries a `README.md` stating this plainly, so a code-first reader does not mistake
deferred scope for abandoned code. If v0.2 slips indefinitely, they get deleted and restored
from history when work resumes — dead weight in the tree is worse than a smaller tree.
