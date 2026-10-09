# OP_RETURN First Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `--op-return` / plan `opReturn` is present, emit a 0-sat OP_RETURN as **vout0** and shift valued outputs (inscription / parent / child / change) by +1, without changing behavior when OP_RETURN is absent.

**Architecture:** Keep sat FI/FO on *valued* outputs only. Prepend nulldata in PSBT builders; update parent validation so “FIFO” means parent=vin0 and vault=first valued vout (index 1 when vout0 value is 0). Wire CLI prints + UI copy to the new indices. Leave commit funding layout unchanged (commit remains vout0 so carrier/target sat @ offset 0 enters commit).

**Tech Stack:** Rust (`phechan-psbt`, `phechan-sat`, `phechan-validation`, `phechan-cli`), local-ui TypeScript/React + Node server (args already pass `--op-return`; mostly copy/index sync).

**Spec:** User brief 2026-10-09 — OP_RETURN-first when present; otherwise keep current layouts.

## Global Constraints

- OP_RETURN value is always `Amount::ZERO` (no user field for value).
- Max payload 80 bytes (unchanged).
- Empty / missing OP_RETURN ⇒ **identical** layouts to today (no leading empty vout).
- Do not change commit PSBT output order (commit stays funding vout0).
- Do not break already-working no-OP_RETURN regtest/mainnet paths.
- Local uncommitted work (parent-child OP_RETURN wiring + Profile payment hardening) must be **kept** and built upon — do not revert it.

## Local vs GitHub (as of plan write)

| Item | State |
|---|---|
| Branch | `main` @ `f9ddaca` — **matches** `origin/main` (0 ahead / 0 behind) |
| Remote | `https://github.com/Blazekachu/phechan.git` |
| Uncommitted | **Yes** — 6 files, +297 / −49 vs HEAD |

Uncommitted files (must remain / fold into this work):

1. `crates/phechan-psbt/src/reveal.rs` — parent-child `op_return` field + append helper + tests (currently **last** output)
2. `crates/phechan-cli/src/cmd_inscription.rs` — passes OP_RETURN into parent-child live/fee paths
3. `crates/phechan-psbt/src/sign_regtest.rs` — sign+finalize with OP_RETURN test
4. `crates/phechan-sat/src/parent.rs` — trailing-zero OP_RETURN sat-flow test
5. `apps/local-ui/src/App.tsx` — payment address pin, network mismatch, Profile fee copy
6. `apps/local-ui/src/commitBundle.ts` — bundle `network` / plan sync

**Pre-step before coding OP_RETURN-first:** either commit that WIP as `fix/feat: parent-child OP_RETURN + control payment pin` or leave working tree dirty and implement on top. Prefer one clean commit of current WIP first so the position change is a second focused commit.

---

## Target layouts

### A) OP_RETURN absent (unchanged)

**Simple / same-sat / sat-target reveal (1-in):**
- vout0 = inscription destination (postage)
- vout1 = change (if any)
- fee = tail

**Parent+child FI/FO (2-in):**
- vout0 = parent vault
- vout1 = child postage
- vout2 = change (if any)

### B) OP_RETURN present (NEW)

**Simple / same-sat / sat-target:**
- vout0 = OP_RETURN (0)
- vout1 = inscription (postage) ← commit input sat @ offset 0
- vout2 = change (if any)
- fee = tail

**Parent+child:**
- vout0 = OP_RETURN (0)
- vout1 = parent vault ← parent vin0 sat @ offset 0
- vout2 = child postage ← commit vin1 sat @ offset 0
- vout3 = change (if any)

**Sat-target / same-sat carrier:** commit funding still puts target sat @ offset 0 into **commit vout0**. Reveal then maps that sat to the first *valued* reveal out = **vout1** when OP_RETURN present.

---

## Files to touch

| File | Role |
|---|---|
| `crates/phechan-psbt/src/reveal.rs` | Prepend OP_RETURN; rewrite unit tests |
| `crates/phechan-sat/src/parent.rs` | Redefine FIFO vault rule for leading 0-value out |
| `crates/phechan-validation/src/parent.rs` | Call sites / tests use `vault_vout: 1` when leading 0 |
| `crates/phechan-cli/src/cmd_inscription.rs` | `vault_vout`, println indices, fee-estimate templates (via builders) |
| `crates/phechan-psbt/src/sign_regtest.rs` | Assert OP_RETURN at `[0]` |
| `crates/phechan-vault/src/lib.rs` | Only if templates assume vault=0 with message outs — audit |
| `apps/local-ui/src/App.tsx` | Copy: parent → vout1 when OP_RETURN, etc. |
| `apps/local-ui/src/api.ts` | Comment on vaultAddress / vout |
| `apps/local-ui/README.md` | One-line note if it mentions output order |

Server `index.mjs` already passes `--op-return`; no arg-order change required.

---

### Task 0: Preserve current WIP

**Files:** the 6 uncommitted files above

- [ ] **Step 1: Review diff**

Run: `git diff --stat origin/main`

- [ ] **Step 2: Commit WIP unchanged (recommended)**

```bash
git add apps/local-ui/src/App.tsx apps/local-ui/src/commitBundle.ts \
  crates/phechan-cli/src/cmd_inscription.rs \
  crates/phechan-psbt/src/reveal.rs crates/phechan-psbt/src/sign_regtest.rs \
  crates/phechan-sat/src/parent.rs
git commit -m "$(cat <<'EOF'
Wire parent-child OP_RETURN and pin reveal change to funding payment address.

EOF
)"
```

(Use PowerShell-safe commit message form on Windows if heredoc unavailable.)

- [ ] **Step 3: Confirm clean tree before position change**

Run: `git status` → clean (or only this plan file untracked)

---

### Task 1: PSBT builders — prepend OP_RETURN

**Files:**
- Modify: `crates/phechan-psbt/src/reveal.rs`
- Test: same file `mod tests`

**Interfaces:**
- Consumes: `RevealPsbtParams.op_return`, `ParentChildRevealParams.op_return`
- Produces: unsigned tx outputs with OP_RETURN at index 0 when payload non-empty

- [ ] **Step 1: Rewrite failing tests for first-output placement**

Replace / add assertions:

```rust
#[test]
fn simple_reveal_op_return_is_vout0() {
    // ... build with op_return: Some(b"hi")
    assert!(psbt.unsigned_tx.output[0].script_pubkey.is_op_return());
    assert_eq!(psbt.unsigned_tx.output[0].value.to_sat(), 0);
    assert_eq!(psbt.unsigned_tx.output[1].value.to_sat(), 546); // dest
}

#[test]
fn parent_child_op_return_is_vout0_vault_vout1_child_vout2() {
    // no change
    assert!(psbt.unsigned_tx.output[0].script_pubkey.is_op_return());
    assert_eq!(psbt.unsigned_tx.output[1].value.to_sat(), 546); // vault
    assert_eq!(psbt.unsigned_tx.output[2].value.to_sat(), 546); // child
}

#[test]
fn parent_child_op_return_then_change_is_vout3() {
    // with change 8000
    assert_eq!(psbt.unsigned_tx.output.len(), 4);
    assert!(psbt.unsigned_tx.output[0].script_pubkey.is_op_return());
    assert_eq!(psbt.unsigned_tx.output[3].value.to_sat(), 8_000);
}

#[test]
fn parent_child_baseline_no_op_return_still_two_outputs() {
    // None / empty → still [vault, child] only
    assert_eq!(psbt.unsigned_tx.output.len(), 2);
    assert!(!psbt.unsigned_tx.output[0].script_pubkey.is_op_return());
}
```

- [ ] **Step 2: Run tests — expect FAIL**

Run: `cargo test -p phechan-psbt reveal::tests -- --nocapture`  
Expected: FAIL on index assertions (still last-output today)

- [ ] **Step 3: Implement prepend helper**

Replace `append_op_return` with `prepend_op_return` (or keep name but insert at front):

```rust
fn prepend_op_return(
    outputs: &mut Vec<TxOut>,
    op_return: Option<Vec<u8>>,
) -> Result<(), PsbtBuildError> {
    let Some(data) = op_return else { return Ok(()); };
    if data.is_empty() { return Ok(()); }
    if data.len() > 80 {
        return Err(PsbtBuildError::Message(
            "OP_RETURN payload exceeds 80-byte standard relay limit".into(),
        ));
    }
    let push: &bitcoin::script::PushBytes = data.as_slice().try_into().map_err(|_| {
        PsbtBuildError::Message("OP_RETURN payload too large for push".into())
    })?;
    outputs.insert(
        0,
        TxOut {
            value: Amount::ZERO,
            script_pubkey: ScriptBuf::new_op_return(push),
        },
    );
    Ok(())
}
```

Call **after** building the valued-output vec (dest/vault/child/change), so insert(0) yields:

`[OP_RETURN, ...valued...]`

Update struct docs to say “optional leading OP_RETURN when present”.

- [ ] **Step 4: Run tests — expect PASS**

Run: `cargo test -p phechan-psbt -- --nocapture`  
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add crates/phechan-psbt/src/reveal.rs
git commit -m "Place reveal OP_RETURN at vout0 when present."
```

---

### Task 2: Sat FIFO policy — vault = first valued output

**Files:**
- Modify: `crates/phechan-sat/src/parent.rs`
- Modify: `crates/phechan-validation/src/parent.rs` (tests / any hard-coded vault=0 with leading 0)
- Test: `crates/phechan-sat/src/parent.rs`

**Interfaces:**
- Consumes: `ParentPlacementPolicy::FirstInFirstOut`, `vault_vout`, `output_values`
- Produces: FIFO allows `vault_vout == 0`, or `vault_vout == 1` when `output_values[0] == 0`

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn fifo_leading_zero_op_return_vault_is_vout1() {
    assert!(verify_parent_return(
        ParentPlacementPolicy::FirstInFirstOut,
        0, // parent vin
        0,
        1, // vault
        &[546, 10_000],
        &[0, 546, 546], // OP_RETURN, vault, child
    )
    .is_ok());
}

#[test]
fn fifo_without_op_return_still_requires_vault_vout0() {
    assert!(verify_parent_return(
        ParentPlacementPolicy::FirstInFirstOut,
        0, 0, 0,
        &[546, 10_000],
        &[546, 546],
    )
    .is_ok());
}
```

Update existing `fifo_trailing_zero_op_return_value_does_not_steal_parent` → either delete or rename to leading-zero case (trailing 0 after valued outs remains OK for Custom; for FIFO with trailing 0 and vault=0 still OK).

- [ ] **Step 2: Run — expect FAIL** on leading-zero vault=1

Run: `cargo test -p phechan-sat parent::tests -- --nocapture`

- [ ] **Step 3: Implement FIFO rule**

```rust
ParentPlacementPolicy::FirstInFirstOut => {
    if parent_input_index != 0 {
        return Err(ParentPlacementError::FifoRequiresParentFirstInput);
    }
    let first_valued = output_values
        .iter()
        .position(|&v| v > 0)
        .ok_or(ParentPlacementError::FifoRequiresVaultFirstOutput)?;
    if vault_vout != first_valued {
        return Err(ParentPlacementError::FifoRequiresVaultFirstOutput);
    }
}
```

Update error Display string to: “FirstInFirstOut requires vault at first valued output”.

- [ ] **Step 4: Fix validation crate tests if they break**

Run: `cargo test -p phechan-validation -p phechan-sat -p phechan-vault -- --nocapture`  
Expected: PASS (vault templates with vault_vout=0 and no leading 0 still pass)

- [ ] **Step 5: Commit**

```bash
git add crates/phechan-sat/src/parent.rs crates/phechan-validation/src/parent.rs
git commit -m "Allow FI/FO vault at first valued output when leading OP_RETURN."
```

---

### Task 3: CLI — vault_vout + logs + live validation arrays

**Files:**
- Modify: `crates/phechan-cli/src/cmd_inscription.rs`
- Modify: `crates/phechan-psbt/src/sign_regtest.rs` (assert output[0] is OP_RETURN)

**Interfaces:**
- When `parse_op_return` is `Some`, parent-child validation uses `vault_vout: 1` and `output_values` starting with `0`.
- Fee estimate already builds via `build_*_reveal_psbt` — picks up prepend automatically.

- [ ] **Step 1: Helper for reveal layout indices**

Near OP_RETURN parsing:

```rust
fn reveal_layout_with_op_return(has_op_return: bool) -> (usize /*vault*/, usize /*child*/, usize /*change*/) {
    if has_op_return { (1, 2, 3) } else { (0, 1, 2) }
}
```

- [ ] **Step 2: Update `reveal_parent_child_wallet`**

- `let has_opr = op_return.as_ref().map(|d| !d.is_empty()).unwrap_or(false);`
- `(vault_i, child_i, change_i) = reveal_layout_with_op_return(has_opr);`
- Build `out_vals`: if has_opr { `vec![0, parent_value, postage]` } else { `vec![parent, postage]` }; push change if any
- `vault_vout: vault_i` in `ParentChildValidationInput`
- Print:
  - `op_return: … → vout0`
  - `vault_vout: {vault_i}`
  - `parent_lands: vout{vault_i} …`
  - `child_lands: vout{child_i} …`
  - change → `vout{change_i}`

Same for regtest `inscription child` live path (`vault_vout: 0` → conditional).

- [ ] **Step 3: Update dry-run / create log line**

Change `"last reveal output"` → `"reveal vout0"`.

- [ ] **Step 4: Fix sign_regtest OP_RETURN test**

```rust
assert!(tx.output[0].script_pubkey.is_op_return());
assert_eq!(tx.output[1].value.to_sat(), 9_500); // or postage dest
```

- [ ] **Step 5: Run CLI + PSBT tests**

Run: `cargo test -p phechan-cli -p phechan-psbt -p phechan-sat -p phechan-validation -- --nocapture`  
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/phechan-cli/src/cmd_inscription.rs crates/phechan-psbt/src/sign_regtest.rs
git commit -m "Sync CLI parent-child validation and logs for OP_RETURN-first."
```

---

### Task 4: UI copy + API comments

**Files:**
- Modify: `apps/local-ui/src/App.tsx` (strings mentioning vout0 parent / vout2 change)
- Modify: `apps/local-ui/src/api.ts` (vaultAddress comment)
- Optional: `apps/local-ui/README.md`

- [ ] **Step 1: Find user-facing index claims**

Grep: `vout0`, `vout1`, `vout2`, `parent returned`

- [ ] **Step 2: Update copy to be accurate when OP_RETURN is used**

Examples:
- Profile help: “With OP_RETURN: vout0 message, vout1 parent, vout2 child, vout3 change (if any).”
- Success line: if `activePlan.opReturn` → parent returned → **vout1**; else **vout0**.
- `api.ts`: `vaultAddress` — “parent return output (vout0, or vout1 when OP_RETURN set)”.

No server arg changes required (`--op-return` already forwarded).

- [ ] **Step 3: Manual smoke (local UI)**

1. Control/fast dry prepare with OP_RETURN text → CLI log shows `→ reveal vout0`
2. Parent+child unsigned PSBT → inspect: output[0] nulldata, [1] vault, [2] child
3. Same without OP_RETURN → vault still output[0]

- [ ] **Step 4: Commit**

```bash
git add apps/local-ui/src/App.tsx apps/local-ui/src/api.ts apps/local-ui/README.md
git commit -m "Update UI copy for OP_RETURN-first reveal layouts."
```

---

### Task 5: Full verification (no regressions)

- [ ] **Step 1: Workspace tests**

Run: `cargo test --workspace -- --nocapture`  
Expected: exit 0; ignored live regtest still ignored

- [ ] **Step 2: Matrix checklist (manual or dry-run)**

| Path | OP_RETURN | Expect |
|---|---|---|
| Simple reveal | no | `[dest][+change]` |
| Simple reveal | yes | `[OP_RETURN, dest][+change]` |
| Parent+child | no | `[vault, child][+change]` |
| Parent+child | yes | `[OP_RETURN, vault, child][+change]` |
| Same-sat / sat-target | yes | commit still holds sat@0; reveal valued out vout1 gets it |
| Fee estimate | yes | vsize includes leading OP_RETURN (via builder) |

- [ ] **Step 3: Do not push until user asks**

Local commits only unless explicitly requested.

---

## Out of scope / non-goals

- Changing commit tx output order
- OP_RETURN value &gt; 0
- Migrating already-confirmed reveals (history may show message last — fine)
- Drop/RBF tooling
- Runestone / OP_13 (different protocol)

## Risk controls

1. **Absent OP_RETURN path untouched in structure** — only prepend when payload non-empty.  
2. **Tests lock both layouts** before/after implementation.  
3. **FIFO policy change is value-based** (`first valued vout`), not a blind `vault_vout = 1` always.  
4. Keep payment-address / Profile hardenings from current WIP — unrelated but already validated mentally.

## Execution order summary

0. Commit current uncommitted WIP  
1. PSBT prepend + unit tests  
2. Sat FIFO first-valued-output rule  
3. CLI vault_vout / logs / sign tests  
4. UI copy  
5. `cargo test --workspace`
