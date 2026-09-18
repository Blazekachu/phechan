# Phase 3 Leftovers — Reinscribe / Delegate / Live Child

> **For agentic workers:** execute task-by-task.

**Goal:** Close Phase 3 gaps: delegate + reinscribe CLI paths, live regtest `child --broadcast`, and document completion on the roadmap.

**Architecture:** Extend `phechan-ordinals` envelopes (tag 11 delegate); CLI subcommands reuse create/reveal commit path; child live adds parent input on reveal with FI/FO return via existing validation.

**Tech Stack:** Existing Rust workspace, regtest RPC.

**Spec:** `docs/protocol-research.md` §3.5; roadmap Phase 3 “suggested later”.

## Global Constraints

- Regtest-only for `--broadcast` (same as create).
- No mainnet; dual gate still on `tx broadcast`.
- Reinscription is append, not overwrite — CLI must say so.
- Delegate may omit body; content resolves via tag 11.
- No Xverse (Phase 5b).

## Tasks

1. Envelope + tapscript: delegate tag; optional empty body
2. CLI `inscription delegate` dry-run + broadcast
3. CLI `inscription reinscribe` (satpoint flag, disclosure, create-like path)
4. Live `inscription child --broadcast` FI/FO on regtest
5. Roadmap Phase 3 checkboxes complete

## Exit

All Phase 3 suggested items done; tests for delegate envelope; child live path exists (may skip if RPC down in CI — unit/dry-run always pass).
