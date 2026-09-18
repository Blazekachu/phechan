# Phase 1 Inscription Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On Bitcoin regtest, build and sign a text inscription create+reveal flow (no parent) with golden envelope tests, PSBT export, and broadcast still dual-gated / validation-aware.

**Architecture:** Extend the existing Cargo workspace. `phechan-ordinals` owns envelope bytes; `phechan-bitcoin` + `phechan-psbt` own Taproot commit output and reveal spend construction via `rust-bitcoin`; `phechan-keystore` signs on regtest; `phechan-validation` reports layers; `phechan-cli` exposes a dry-run / regtest-only create path. No parent, no Runes etch, no launchpad.

**Tech Stack:** Rust 2021, `rust-bitcoin` (0.32+ with Taproot/PSBT), `secp256k1`, existing Phechan crates under `F:\Users\akhil\Main\phechan`.

**Spec:** `docs/superpowers/specs/2026-09-17-phechan-phase0-design.md` and `docs/architecture.md`, `docs/protocol-research.md`, `docs/implementation-roadmap.md` (Phase 1 only).

## Global Constraints

- Inscription tool only — no Runes name-commitment / etch product flow.
- No forced multi-block wait for plain inscriptions.
- Do not implement parent preserve in this plan (Phase 2).
- Do not implement batch vault / ACP spikes in this plan.
- Mainnet broadcast: `PHECHAN_ALLOW_MAINNET_BROADCAST=1` **and** typed `BROADCAST MAINNET` **and** passing validation.
- Prefer plain UTXOs for fees; asset disclosure hooks may be stubs until Phase 2.
- Windows: ensure MSVC Build Tools (`link.exe`) or a working Rust linker before claiming green builds.
- Commit only when the human asks, or at task commit steps if the human already approved executing with commits; otherwise leave working tree dirty and report.
- YAGNI: text inscriptions only (`text/plain;charset=utf-8`).

---

## File map (Phase 1)

| Path | Role |
|---|---|
| `crates/phechan-ordinals/src/envelope.rs` | Envelope encode/decode helpers |
| `crates/phechan-ordinals/src/script_push.rs` | Minimal Bitcoin script push compile |
| `crates/phechan-ordinals/src/tapscript.rs` | `<xonly> OP_CHECKSIG` + envelope leaf |
| `crates/phechan-ordinals/tests/golden_envelope.rs` | Golden vectors |
| `crates/phechan-bitcoin/src/network.rs` | Keep/extend Network |
| `crates/phechan-bitcoin/src/taproot_commit.rs` | P2TR address / script pubkey from leaf |
| `crates/phechan-bitcoin/Cargo.toml` | Add `bitcoin`, `secp256k1` deps |
| `crates/phechan-keystore/src/regtest.rs` | Deterministic regtest keypair |
| `crates/phechan-psbt/src/commit.rs` | Unsigned PSBT creating commit output |
| `crates/phechan-psbt/src/reveal.rs` | Script-path reveal PSBT |
| `crates/phechan-validation/src/inscription.rs` | Basic structural checks |
| `crates/phechan-cli/src/main.rs` | `inscription create --dry-run` / regtest helpers |
| `tests/regtest_inscription.rs` | Optional bitcoind e2e (feature-gated) |
| `docs/implementation-roadmap.md` | Mark Phase 1 items as done when exit met |

---

### Task 1: Fix Windows link toolchain (blocker)

**Files:**
- Modify: none required if linker already works
- Create: `docs/dev-setup-windows.md` only if linker setup steps are needed

**Interfaces:**
- Consumes: none
- Produces: `cargo build -p phechan-cli` succeeds on the executor machine

- [ ] **Step 1: Probe linker**

Run (PowerShell):

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build -p phechan-cli --manifest-path F:\Users\akhil\Main\phechan\Cargo.toml
```

Expected: either success, or `linker link.exe not found`.

- [ ] **Step 2: If link failed, install/fix MSVC**

Install “Build Tools for Visual Studio” with Desktop C++ workload, open a fresh shell, or use Developer PowerShell. Re-run Step 1 until link succeeds.

- [ ] **Step 3: Document the fix**

If setup was non-obvious, write `docs/dev-setup-windows.md` with the exact steps that worked. If already green, skip the doc.

- [ ] **Step 4: Verify tests compile**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p phechan-cli --manifest-path F:\Users\akhil\Main\phechan\Cargo.toml
```

Expected: PASS (existing phase0 tests).

---

### Task 2: Script push compiler + failing golden envelope test

**Files:**
- Create: `crates/phechan-ordinals/src/script_push.rs`
- Create: `crates/phechan-ordinals/src/envelope.rs`
- Modify: `crates/phechan-ordinals/src/lib.rs`
- Create: `crates/phechan-ordinals/tests/golden_envelope.rs`

**Interfaces:**
- Consumes: none
- Produces:
  - `pub fn compile_script(chunks: &[ScriptChunk]) -> Vec<u8>`
  - `pub fn build_text_envelope(body: &[u8]) -> Vec<u8>`
  - `ScriptChunk` enum: `Op(u8)` | `Data(Vec<u8>)`

- [ ] **Step 1: Write the failing golden test**

Create `crates/phechan-ordinals/tests/golden_envelope.rs`:

```rust
use phechan_ordinals::envelope::build_text_envelope;

#[test]
fn hello_world_envelope_matches_handbook_shape() {
    let script = build_text_envelope(b"Hello, world!");
    // Must contain ASCII "ord" and content type and body.
    let ord = b"ord";
    assert!(script.windows(ord.len()).any(|w| w == ord));
    let ctype = b"text/plain;charset=utf-8";
    assert!(script.windows(ctype.len()).any(|w| w == ctype));
    assert!(script.windows(13).any(|w| w == b"Hello, world!"));
    // Envelope markers
    assert_eq!(script[0], 0x00); // OP_FALSE
    assert_eq!(script[1], 0x63); // OP_IF
    assert_eq!(*script.last().unwrap(), 0x68); // OP_ENDIF
}
```

- [ ] **Step 2: Run test to verify it fails**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p phechan-ordinals --manifest-path F:\Users\akhil\Main\phechan\Cargo.toml --test golden_envelope
```

Expected: FAIL (module/function missing).

- [ ] **Step 3: Implement minimal `script_push` + `envelope`**

`script_push.rs`: BIP62.3-style minimal pushes for empty, OP_1..OP_16, OP_1NEGATE, and PUSHDATA1/2 as needed; max push 520 for body chunking later.

`envelope.rs`:

```rust
pub fn build_text_envelope(body: &[u8]) -> Vec<u8> {
    // OP_FALSE OP_IF
    //   push "ord"
    //   push 1 / push content-type
    //   push 0 / push body chunks
    // OP_ENDIF
}
```

Wire `mod script_push; pub mod envelope;` from `lib.rs`. Keep `PROTOCOL_TAG`.

- [ ] **Step 4: Run test to verify it passes**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p phechan-ordinals --manifest-path F:\Users\akhil\Main\phechan\Cargo.toml --test golden_envelope
```

Expected: PASS.

- [ ] **Step 5: Add chunking unit test for body > 520 bytes**

In `envelope.rs` `#[cfg(test)]`, assert a 521-byte body yields two data pushes after the body tag.

- [ ] **Step 6: Commit (if human approved commits)**

```bash
git add crates/phechan-ordinals
git commit -m "$(cat <<'EOF'
feat(ordinals): add text inscription envelope encoder with golden test

EOF
)"
```

---

### Task 3: Tapscript leaf (`OP_CHECKSIG` + envelope)

**Files:**
- Create: `crates/phechan-ordinals/src/tapscript.rs`
- Modify: `crates/phechan-ordinals/src/lib.rs`
- Modify: `crates/phechan-ordinals/Cargo.toml` (add `bitcoin` if using `XOnlyPublicKey` bytes only — prefer raw `[u8; 32]` to avoid dep churn here)

**Interfaces:**
- Consumes: `build_text_envelope`
- Produces: `pub fn build_inscription_tapscript(internal_xonly: &[u8; 32], body: &[u8]) -> Vec<u8>`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn tapscript_starts_with_32byte_key_and_checksig() {
    let key = [0x02u8; 32];
    let script = build_inscription_tapscript(&key, b"hi");
    assert_eq!(&script[0..32], &key);
    assert_eq!(script[32], 0xac); // OP_CHECKSIG
    assert_eq!(script[33], 0x00); // OP_FALSE of envelope
}
```

- [ ] **Step 2: Run — expect FAIL**

- [ ] **Step 3: Implement by concatenating `push32(key) OP_CHECKSIG` + envelope bytes (envelope must NOT be data-pushed)**

- [ ] **Step 4: Run — expect PASS**

- [ ] **Step 5: Commit (if approved)**

```bash
git add crates/phechan-ordinals
git commit -m "$(cat <<'EOF'
feat(ordinals): compose inscription tapscript leaf

EOF
)"
```

---

### Task 4: Add `rust-bitcoin` to workspace and Taproot commit helper

**Files:**
- Modify: `Cargo.toml` (workspace deps)
- Modify: `crates/phechan-bitcoin/Cargo.toml`
- Create: `crates/phechan-bitcoin/src/taproot_commit.rs`
- Modify: `crates/phechan-bitcoin/src/lib.rs`

**Interfaces:**
- Consumes: tapscript bytes + internal key
- Produces:
  - `pub struct CommitOutput { pub script_pubkey: ScriptBuf, pub address: Address, pub spend_info: ... }`
  - `pub fn build_commit_output(secp, network, internal_key, leaf_script) -> Result<CommitOutput, Error>`

Pin exact versions in workspace.dependencies, e.g.:

```toml
bitcoin = { version = "0.32.5", features = ["std"] }
secp256k1 = { version = "0.29", features = ["global-context", "rand"] }
```

(Adjust to latest compatible 0.32.x available at implement time; pin exact.)

- [ ] **Step 1: Add dependencies; `cargo check -p phechan-bitcoin`**

- [ ] **Step 2: Write failing test that commit address is P2TR on regtest**

```rust
#[test]
fn commit_output_is_p2tr_regtest() {
    // fixed key + leaf "hello" -> Address::p2tr...
    assert!(addr.to_string().starts_with("bcrt1p"));
}
```

- [ ] **Step 3: Implement using `bitcoin::taproot::TaprootBuilder` + `Address::p2tr`**

- [ ] **Step 4: Tests PASS**

- [ ] **Step 5: Commit (if approved)**

```bash
git add Cargo.toml crates/phechan-bitcoin
git commit -m "$(cat <<'EOF'
feat(bitcoin): build Taproot commit output for inscription leaf

EOF
)"
```

---

### Task 5: Regtest keystore (deterministic)

**Files:**
- Modify: `crates/phechan-keystore/Cargo.toml`
- Create: `crates/phechan-keystore/src/regtest.rs`
- Modify: `crates/phechan-keystore/src/lib.rs`

**Interfaces:**
- Consumes: `Network` (reject Mainnet keygen — already stubbed)
- Produces:
  - `pub struct RegtestKey { pub secret: SecretKey, pub xonly: XOnlyPublicKey }`
  - `pub fn derive_regtest_key(seed_label: &str) -> Result<RegtestKey, Error>` using a fixed test seed domain string (e.g. SHA256(`phechan-regtest/` || label)) — **not** BIP39 yet.

- [ ] **Step 1: Failing test — same label ⇒ same xonly; Mainnet ⇒ error**

- [ ] **Step 2: Implement**

- [ ] **Step 3: PASS**

- [ ] **Step 4: Commit (if approved)**

```bash
git add crates/phechan-keystore
git commit -m "$(cat <<'EOF'
feat(keystore): deterministic regtest key derivation

EOF
)"
```

---

### Task 6: Commit + reveal PSBT builders (unsigned)

**Files:**
- Modify: `crates/phechan-psbt/Cargo.toml`
- Create: `crates/phechan-psbt/src/commit.rs`
- Create: `crates/phechan-psbt/src/reveal.rs`
- Modify: `crates/phechan-psbt/src/lib.rs`

**Interfaces:**
- Consumes: funding outpoint+value+script, commit output value, reveal fee estimate, inscription leaf, control block
- Produces:
  - `build_commit_psbt(...) -> Psbt`
  - `build_reveal_psbt(...) -> Psbt` spending commit via script path to a single destination output (inscription sat on first sat of reveal input → first output of equal-or-correct value)

**Important:** Reveal output value must keep the inscribed sat out of fees (Phase 1: single reveal input, single destination output + optional change only if carefully valued; simplest path: one output = commit_value - fee, no change).

- [ ] **Step 1: Unit test serialize PSBT and assert 1 output commit / 1 input reveal with tap leaf script present**

- [ ] **Step 2: Implement minimal builders**

- [ ] **Step 3: PASS**

- [ ] **Step 4: Commit (if approved)**

```bash
git add crates/phechan-psbt
git commit -m "$(cat <<'EOF'
feat(psbt): unsigned commit and reveal builders for text inscriptions

EOF
)"
```

---

### Task 7: Keystore sign + finalize reveal (regtest)

**Files:**
- Create: `crates/phechan-psbt/src/sign_regtest.rs` **or** keep signing in `phechan-keystore` and finalize in `phechan-psbt`
- Prefer: `phechan-keystore` signs Taproot script-path; `phechan-psbt` finalizes witness

**Interfaces:**
- `sign_reveal_script_path(psbt, key, leaf, control_block) -> Psbt`
- `finalize(psbt) -> Transaction`

- [ ] **Step 1: Test — sign+finalize yields consensus-encodeable tx with non-empty witness**

- [ ] **Step 2: Implement with `rust-bitcoin` sighash + schnorr**

- [ ] **Step 3: PASS**

- [ ] **Step 4: Commit (if approved)**

```bash
git add crates/phechan-psbt crates/phechan-keystore
git commit -m "$(cat <<'EOF'
feat: regtest script-path sign and finalize for reveal

EOF
)"
```

---

### Task 8: Validation report for inscription create (structural)

**Files:**
- Create: `crates/phechan-validation/src/inscription.rs`
- Modify: `crates/phechan-validation/src/lib.rs`

**Interfaces:**
- `pub fn validate_inscription_reveal(tx: &Transaction, expected_body: &[u8]) -> ValidationReport`

Checks (Phase 1):
- tx has ≥1 input, ≥1 output
- witness of reveal input contains envelope bytes / `ord` marker
- output value > 0 and not dust for P2TR (use conservative dust constant documented as policy, not consensus)
- `allows_broadcast()` true only on regtest/signet/testnet when structural checks pass; mainnet still requires dual gate at CLI

- [ ] **Step 1: Failing tests for missing envelope ⇒ errors; happy path ⇒ allows_broadcast on regtest**

- [ ] **Step 2: Implement**

- [ ] **Step 3: PASS**

- [ ] **Step 4: Update `phase0_blocked` to remain for unimplemented paths; CLI uses new validator for inscription reveal**

- [ ] **Step 5: Commit (if approved)**

```bash
git add crates/phechan-validation
git commit -m "$(cat <<'EOF'
feat(validation): structural inscription reveal report

EOF
)"
```

---

### Task 9: CLI `inscription create` dry-run

**Files:**
- Modify: `crates/phechan-cli/src/main.rs` (split into `cli.rs` / `cmd_inscription.rs` if file grows)
- Modify: `crates/phechan-cli/Cargo.toml` (depend on ordinals, psbt, keystore, bitcoin)

**Interfaces:**
- `phechan inscription create --body "Hello, world!" --network regtest --dry-run`
- Prints: commit address, envelope hex length, reveal tx hex (if keys/funding mocked), validation report
- Does **not** broadcast unless `--broadcast` and gates pass

For Phase 1 without live bitcoind, dry-run may construct commit+reveal against a **mock funding outpoint** and still sign with keystore for local verification.

- [ ] **Step 1: Help text lists `inscription create`**

- [ ] **Step 2: Dry-run path builds envelope + commit address + signed reveal (mock fund) + validation**

- [ ] **Step 3: Manual run**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -p phechan-cli --manifest-path F:\Users\akhil\Main\phechan\Cargo.toml -- inscription create --body "Hello, world!" --network regtest --dry-run
```

Expected: prints commit `bcrt1p...`, validation ok, no broadcast.

- [ ] **Step 4: `tx broadcast` still blocked without validation pass + gates**

- [ ] **Step 5: Commit (if approved)**

```bash
git add crates/phechan-cli
git commit -m "$(cat <<'EOF'
feat(cli): inscription create dry-run on regtest

EOF
)"
```

---

### Task 10: Optional live regtest e2e (feature-gated)

**Files:**
- Create: `tests/regtest_inscription.rs`
- Modify: root `Cargo.toml` or `crates/phechan-cli` with `[[test]]` and feature `regtest-e2e`

**Interfaces:**
- Ignored by default: `#[ignore]` or require `PHECHAN_REGTEST_RPC=http://127.0.0.1:18443`

- [ ] **Step 1: If `F:\Users\akhil\Main\regtest-stack` (or user RPC) is up, fund keystore address, broadcast commit, broadcast reveal, assert getrawtransaction works**

- [ ] **Step 2: If RPC unavailable, leave test `#[ignore]` with README note — do not fail CI**

- [ ] **Step 3: Commit (if approved)**

```bash
git add tests docs
git commit -m "$(cat <<'EOF'
test: optional regtest e2e for text inscription create/reveal

EOF
)"
```

---

### Task 11: Phase 1 exit checklist + roadmap update

**Files:**
- Modify: `docs/implementation-roadmap.md`
- Modify: `README.md` (how to dry-run)

- [ ] **Step 1: Confirm exit criteria**

- Golden envelope test PASS  
- Tapscript + P2TR commit address PASS  
- Sign+finalize reveal PASS  
- `inscription create --dry-run` works  
- Mainnet broadcast still dual-gated  
- No parent / no runes etch code paths shipped as product  

- [ ] **Step 2: Mark Phase 1 boxes in roadmap; leave Phase 2 unchecked**

- [ ] **Step 3: Short note in README under Quick start**

- [ ] **Step 4: Commit (if approved)**

```bash
git add docs README.md
git commit -m "$(cat <<'EOF'
docs: mark phase 1 inscription core complete

EOF
)"
```

---

## Self-review (plan vs spec)

| Spec / roadmap item | Task |
|---|---|
| Envelope builder (text) | Task 2–3 |
| Taproot create + reveal | Task 4, 6–7 |
| PSBT create/inspect/finalize | Task 6–7 (inspect can be debug print in CLI) |
| Validation report | Task 8 |
| Regtest keystore sign | Task 5, 7 |
| Golden envelope tests | Task 2 |
| No parent / no etch product | Global constraints |
| Mainnet dual gate preserved | Task 9 + existing CLI |
| Phase 2 parent explicitly excluded | Stated throughout |

No TBD placeholders in task steps. Types named above are the contracts later tasks rely on.

---

## Out of scope (do not sneak in)

- Parent FI/FO / sat-flow simulator (Phase 2 plan)
- UTXO indexer labeling productization (Phase 2)
- Vanity grinder
- Local UI
- Runes etch ceremony
