# Phase 2 Parent Preserve + Asset Disclosure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove parent sat returns to a vault output via sat-flow simulation, with UTXO asset disclosure and default first-in/first-out parent layout (opt-in custom layouts if simulation passes).

**Architecture:** Extend `phechan-sat` with ordinal sat-flow; extend envelopes with parent tag; label/disclose UTXO assets; validate parent+child reveal layouts before sign. No Runes etch product; rune-bearing inputs warn/block silent burns.

**Tech Stack:** Existing Phechan Rust workspace, `rust-bitcoin` 0.32.

**Spec:** `docs/architecture.md` §3–6, `docs/implementation-roadmap.md` Phase 2, `docs/protocol-research.md` §3.4.

## Global Constraints

- Default parent layout: first input / first output.
- Opt-in layouts only if sat-flow simulation passes.
- Prefer plain UTXOs for fees; never silently fee-spend inscribed/rune UTXOs.
- Rune-bearing spend without validated runestone ⇒ block (warn + error), not silent continue.
- No Runes etch ceremony. No launchpad.
- Mainnet dual gate unchanged.
- MinGW PATH may be required on this Windows host (`docs/dev-setup-windows.md`).

---

## File map

| Path | Role |
|---|---|
| `crates/phechan-sat/src/flow.rs` | Sat-flow simulator |
| `crates/phechan-sat/src/parent.rs` | Placement templates + verify |
| `crates/phechan-runes/src/disclosure.rs` | Asset labels + disclosure lines |
| `crates/phechan-ordinals/src/envelope.rs` | Parent tag `3` support |
| `crates/phechan-ordinals/src/parent_id.rs` | Encode inscription id for tag 3 |
| `crates/phechan-validation/src/parent.rs` | Parent return + fee-tail checks |
| `crates/phechan-psbt/src/parent_child.rs` | Reveal layout helpers (values/order) |
| `crates/phechan-cli/src/main.rs` | `inscription child --parent … --dry-run` |

---

### Task 1: Sat-flow simulator

**Produces:** `simulate_sat_flow(input_values, output_values) -> SatFlowResult` with `location_of(global_offset) -> Output(vout) | Fee`

- [ ] Failing tests: 1000+2000 in → 1000+1500 out ⇒ offset 0 → vout0; offset 1000 → vout1; offset 2500 → Fee; parent fee-tail case
- [ ] Implement FIFO assignment
- [ ] PASS

### Task 2: Parent placement policies

**Produces:** `verify_parent_return(policy, parent_input_index, parent_sat_offset, vault_vout, inputs, outputs) -> Result<()>`

- [ ] FI/FO happy path PASS; parent as 2nd input with only trailing dust out → Fee FAIL
- [ ] Custom last-in/first-out PASS when values work

### Task 3: UTXO asset disclosure

**Produces:** `format_disclosure(utxos: &[LabeledUtxo]) -> Vec<String>`; block helper `fee_input_allowed`

- [ ] Tests for plain / inscription / rune / unknown messaging

### Task 4: Parent tag in envelope

**Produces:** `build_text_envelope_with_parent(body, parent_id)`; `encode_inscription_id`

- [ ] Golden: tag 3 bytes present for known id `…i0`

### Task 5: Validation + fee-tail regression

**Produces:** `validate_parent_child_reveal(...)`

- [ ] Regression: parent sat in fee ⇒ errors, `allows_broadcast=false`
- [ ] FI/FO layout ⇒ ok

### Task 6: CLI child dry-run

- [ ] `phechan inscription child --body … --parent <id> --dry-run` prints disclosure + sat-flow + commit address
- [ ] Update roadmap Phase 2 checkboxes

---

## Out of scope

- Live indexer HTTP client (accept manually labeled UTXOs / CLI flags)
- Full Runestone encode
- Batch vault / ACP
