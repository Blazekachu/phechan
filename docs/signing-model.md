# Phechan Signing Model

Date: 2026-09-17  
Status: v1 model approved; V2 spike matrix **complete** (Taproot key-path unit tests)

## 1. v1 model (inscription tool)

### What we do

1. Engine builds a Bitcoin transaction and PSBT.  
2. Signing paths:  
   - **Regtest keystore** — deterministic automated tests / CI.  
   - **External wallet** — PSBT export → sign → import → finalize.  
3. Default sighash for single-party flows: **`SIGHASH_ALL`** (Taproot Default/All).  
4. After any signature material is applied, Phechan re-validates the **final serialized transaction** (sat-flow, dust/relay, asset disclosure constraints)—not the PSBT’s friendly labels.

### Guarantees (v1)

- A completed `SIGHASH_ALL` signature commits the signer to that exact input set and output set (for that input’s signature).  
- Broadcast requires validation report pass + network gates.  
- Mainnet additionally requires env unlock and typed confirmation.

### Non-guarantees (v1)

- Does not by itself prevent a user from signing a harmful tx if they override warnings.  
- Does not provide multi-party mint allocation binding.  
- Does not make mempool replacement impossible.

## 2. Spike results (regtest-shaped unit tests)

Harness: `crates/phechan-psbt/src/sighash_spikes.rs`  
Run: `cargo test -p phechan-psbt spike_`

Mutations checked after one key-path signature on input 0:

| Mutation | ALL / Default | ALL\|ACP | SINGLE\|ACP |
|---|---|---|---|
| Add input | fail | **pass** | **pass** |
| Append parent input | fail | **pass** | **pass** |
| Add output (matched out unchanged) | fail | fail | **pass** |
| Reorder outputs | fail | fail | fail |
| Change matched / committed out value (fee) | fail | fail | fail |
| Swap matched / committed out script | fail | fail | fail |
| Wrong prevout value | fail | fail | fail |

### Folklore disproved

- **“ACP lets the coordinator freely edit outputs”** — false for `ALL|ACP`. Outputs (values + scripts + order) stay bound. ACP only relaxes **which other inputs** may be attached.
- **`SINGLE|ACP` is not a free-form template.** It binds input *i* to output *i* only. Extra outputs are allowed; moving or mutating the matched output breaks the sig. Other parties can still insert unconstrained outputs unless the protocol separately binds them.

## 3. V2 recommendation (parent vault / batch)

**Selected model (paper design):** `SIGHASH_ALL | ANYONECANPAY` for **minter** contributions when the **full output template is fixed before minter sign**.

| Role | Sighash | Why |
|---|---|---|
| Minter (child / payment inputs) | `ALL\|ACP` | Can append fee/parent/other inputs; cannot rewrite mint outputs |
| Parent vault / coordinator (if co-signing) | Prefer `ALL`, or MuSig2 / script policy later | Parent return + fee policy must not be editable after vault sign |
| Single-party inscription (v1) | Keep `ALL` | Simplest; no multi-party need |

**When to consider `SINGLE|ACP`:** only if each minter must bind *exactly one* output pair and the coordinator is trusted (or separately constrained) for all other outs. Not the default for Phechan vault.

**Does not replace:** final tx re-validation, reservation locks, and parent-return checks in `protocols/parent-vault/`.

## 4. Statement (filled after spikes)

> **Selected model:** Minter `SIGHASH_ALL|ANYONECANPAY` over a **pre-committed output template**; vault/parent prefer `SIGHASH_ALL` or stronger co-sign; v1 stays on `SIGHASH_ALL`.  
> **Guarantees:** Minter ACP sig survives added inputs (fee bump / parent append) while binding the entire output set. ALL binds both input and output sets for that signer.  
> **Does not guarantee:** Output edits under ACP; mempool RBF safety; honest coordinator without additional vault policy; wallet UI metadata fidelity (always re-verify final tx).  
> **Residual coordinator trust:** Must not change outputs after minters sign; must return parent per state machine; fee/input assembly is coordinator-driven under ACP.

Until Phase 6 implements vault + batch, V2 product must not ship.

## 5. Phase 5b — Wallet connect + signet (landed)

- **CLI:** `inscription create --unsigned-psbt` (dry or live-funded); `psbt finalize-import --base64 … --expect-body … [--broadcast] --network regtest|signet`  
- **UI:** Wallet / PSBT tab — Connect Xverse, Sign PSBT, paste finalize, typed broadcast confirm  
- **Signet:** Point `PHECHAN_RPC_*` at signet bitcoind; no local mine after broadcast  
- **Soak checklist (operator):** fund signet → unsigned PSBT → Xverse sign → finalize-import → confirm in `ord` / explorer  

v1 keystore path remains for CI/regtest.

## 6. Required spike matrix (status)

- [x] Add inputs  
- [x] Add outputs  
- [x] Change output order  
- [x] Change fees (matched output value)  
- [x] Add parent input (append)  
- [x] Malicious output substitution  
- [x] Invalid / wrong prevout  
- [~] Transaction replacement — RBF signaling orthogonal; sig binding covered above; full RBF policy deferred to ops docs  

**Pass criteria met:** guarantees documented; ACP output-edit folklore disproved.
