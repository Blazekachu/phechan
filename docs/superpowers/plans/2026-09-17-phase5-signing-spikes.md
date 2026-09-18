# Phase 5 Signing Spikes + Parent Vault Design

> **For agentic workers:** executing-plans / implement task-by-task.

**Goal:** Prove or disprove multi-party sighash candidates on regtest-shaped unit tests, document guarantees in `signing-model.md`, and write the parent-vault state machine design. Defer wallet connect / Xverse signet.

**Architecture:** Add `phechan-psbt` (or `tests/`) sighash spike tests using `rust-bitcoin` Taproot/segwit signing helpers; update docs; no launchpad product code.

**Tech Stack:** Existing Rust workspace, `rust-bitcoin` 0.32.

**Spec:** `docs/signing-model.md`, `docs/architecture.md` V2, roadmap Phase 5.

## Global Constraints

- Do not ship batch coordinator product.
- Do not add Xverse / wallet-connect in this phase (noted as Phase 5b later).
- Each candidate must document: guarantees / does not guarantee.
- Prefer automated tests over manual RPC where possible.

## Deferred (explicit)

- Wallet connect (Xverse) + signet support — after Phase 5 docs land.

## Tasks

1. Spike harness: build tx, sign input with chosen sighash, mutate tx, check sig validity
2. Matrix: ALL, ALL|ACP, SINGLE|ACP — add input, add output, reorder outs, fee change, parent add, malicious out swap
3. Update `docs/signing-model.md` with selected recommendation for next prototype
4. Write `protocols/parent-vault/STATE_MACHINE.md`
5. Roadmap checkboxes + note Phase 5b wallet/signet

## Exit

Founder can read what each sighash guarantees; vault states are designed on paper; no false claim that ACP allows free output edits.
