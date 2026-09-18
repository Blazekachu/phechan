# Phase 6 — Batch launchpad (regtest vault)

> **For agentic workers:** execute task-by-task.

**Goal:** Implement parent-vault state machine (in-memory/file), mint reservation, sealed output template, parent-return validator hook, anti-sniping binding design doc. Regtest/demo only — no production parent funds.

**Architecture:** New crate `phechan-vault` encoding `STATE_MACHINE.md`; CLI `vault` / `batch` commands; reuse `validate_parent_child_layout` + `ALL|ACP` docs. Anti-sniping = reservation + sealed template (design doc).

**Tech Stack:** Rust workspace, serde JSON persistence under `.phechan/vault/`.

**Spec:** `protocols/parent-vault/STATE_MACHINE.md`, `docs/signing-model.md`.

## Global Constraints

- No mainnet vault.
- Prefer Phase 5b wallet path before any non-regtest demo (signet optional later).
- Do not ship public launchpad UI in this phase — CLI + library first.

## Tasks

1. Crate `phechan-vault`: states, transitions, reservation, seal template
2. Unit tests for illegal transitions + freeze outputs after ASSEMBLED
3. CLI `vault status|fund|open-batch|reserve|seal|abort`
4. Wire parent-return check before BROADCAST transition
5. `protocols/parent-vault/ANTI_SNIPING.md` design
6. Roadmap Phase 6 checkboxes

## Exit

Can drive vault states on disk via CLI; parent return enforced; anti-sniping written.
