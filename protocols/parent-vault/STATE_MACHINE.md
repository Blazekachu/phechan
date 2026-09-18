# Parent vault — state machine (design)

Status: design-only (Phase 5). No production parent funds.  
Depends on: `docs/signing-model.md` V2 selection (`ALL|ACP` minters + fixed output template).

## 1. Goals

1. Hold a **parent inscription UTXO** (and optional fee UTXOs) for batch child mints.  
2. Ensure every broadcast batch **returns the parent** to a vault-controlled output (FI/FO default).  
3. Never require minters to see or move the parent key material.  
4. Fail closed: no broadcast if parent return / sat-flow / asset disclosure fails.

## 2. Actors

| Actor | Holds | Signs |
|---|---|---|
| Vault operator / coordinator | Parent UTXO (+ fee coins), batch PSBT assembly | Parent input (`SIGHASH_ALL` preferred) |
| Minters | Child funding UTXOs | Own inputs with `ALL\|ACP` over fixed outs |
| Phechan engine | Policy + validation | Does not custody mainnet keys in v1 |

## 3. States

```
                    ┌─────────────┐
                    │  UNFUNDED   │
                    └──────┬──────┘
                           │ deposit parent (+ optional fee)
                           ▼
                    ┌─────────────┐
              ┌────►│   READY     │◄────────────────┐
              │     └──────┬──────┘                 │
              │            │ open_batch(template)   │ abort / expire
              │            ▼                        │
              │     ┌─────────────┐                 │
              │     │ RESERVING   │─────────────────┘
              │     └──────┬──────┘
              │            │ reservations filled or timeout→partial
              │            ▼
              │     ┌─────────────┐
              │     │  ASSEMBLED  │  outputs fixed; PSBTs to minters
              │     └──────┬──────┘
              │            │ collect minter ALL|ACP sigs
              │            ▼
              │     ┌─────────────┐
              │     │ PARTIAL_SIG │
              │     └──────┬──────┘
              │            │ vault signs parent; finalize; validate
              │            ▼
              │     ┌─────────────┐
              │     │  BROADCAST  │──fail──► READY (or QUARANTINE)
              │     └──────┬──────┘
              │            │ confirmed
              │            ▼
              │     ┌─────────────┐
              └─────│  SETTLED    │  parent outpoint updated
                    └─────────────┘

Terminal / hold:
  QUARANTINE — parent still vault-owned but policy breached / stuck tx
  CLOSED     — operator withdrew parent; no further batches
```

### State meanings

| State | Parent UTXO | Allowed actions |
|---|---|---|
| `UNFUNDED` | none | Fund parent (and fee) into vault address |
| `READY` | known, idle | `open_batch`, withdraw (`CLOSED`), inspect |
| `RESERVING` | locked to batch id | Accept mint requests until cap/timeout; cancel → `READY` |
| `ASSEMBLED` | locked | Output template **frozen**; distribute PSBTs; no output edits |
| `PARTIAL_SIG` | locked | Ingest minter sigs; reject malformed / wrong sighash |
| `BROADCAST` | spent in mempool/chain | Wait confirm; RBF only under documented policy |
| `SETTLED` | new vault outpoint | Record provenance; → `READY` |
| `QUARANTINE` | ambiguous / conflicted | Manual resolve; no auto-batch |
| `CLOSED` | withdrawn | Terminal |

## 4. Transitions (normative)

1. **fund** — `UNFUNDED` → `READY` when parent outpoint + script known and asset disclosure clean.  
2. **open_batch(batch_id, output_template, caps)** — `READY` → `RESERVING`. Template includes parent-return output index and child outs.  
3. **reserve(minter_id, utxo_commitment)** — stay `RESERVING`; reject double-spend of same funding outpoint.  
4. **seal_template** — `RESERVING` → `ASSEMBLED` when filled or operator seals early. **After this, outputs are immutable.**  
5. **collect_sig** — `ASSEMBLED` → `PARTIAL_SIG` as minter `ALL|ACP` signatures arrive; verify against sealed template.  
6. **finalize_and_validate** — vault adds parent input (ACP allows append), signs parent with `ALL` (or co-sign scheme), builds final tx, runs sat-flow + parent-return + dust/relay + disclosure. Fail → stay `PARTIAL_SIG` or abort to `READY` if unbroadcast.  
7. **broadcast** — `PARTIAL_SIG` → `BROADCAST` only after validation pass + network gates.  
8. **confirm** — `BROADCAST` → `SETTLED` → `READY` with new parent outpoint.  
9. **abort_batch** — `RESERVING`/`ASSEMBLED`/`PARTIAL_SIG` (pre-broadcast) → `READY`.  
10. **quarantine** — any conflicted spend / unexpected parent move → `QUARANTINE`.  
11. **close** — `READY` → `CLOSED` after intentional parent withdrawal.

## 5. Invariants

1. **Parent return:** Every `BROADCAST` candidate must include a vault-controlled output carrying the parent sat (default FI/FO; opt-in layouts only if sat-flow validator passes).  
2. **Output freeze:** After `ASSEMBLED`, no party may alter outputs; minter ACP proves they cannot either.  
3. **No output folklore:** Coordinator must not claim ACP allows rewriting mint allocations.  
4. **Single active batch** per parent outpoint (v1 vault simplicity).  
5. **No mainnet vault** until signet soak + signing doc + dual gate (see roadmap).  
6. **Disclosure:** Parent + any Runes/rare sats on co-spent inputs must surface in validation report before vault sign.

## 6. Signing alignment

- Minters: `SIGHASH_ALL|ANYONECANPAY` on sealed template (see `docs/signing-model.md`).  
- Vault parent input: `SIGHASH_ALL` after all minter sigs collected (binds final input set including parent).  
- Alternative later: MuSig2 / Taproot script path — out of Phase 5 scope.

## 7. Failure / timeout policy (sketch)

| Event | Action |
|---|---|
| Reservation timeout | Drop incomplete slots; seal or abort |
| Minter never signs | Exclude input; if fee insufficient → abort |
| Validation fail pre-broadcast | Abort batch; parent remains `READY` |
| Broadcast, never confirms | Operator policy: wait / RBF fee package / quarantine |
| Parent sniped externally | `QUARANTINE`; halt batches |

## 8. Non-goals (this doc)

- Product UI for launchpad  
- Xverse / wallet-connect (Phase 5b)  
- Anti-sniping cryptography beyond reservation + sealed template  
- Multi-parent / concurrent batches  

## 9. Exit for implementation (Phase 6)

Implement only after: spike doc signed off, this state machine reviewed, and Phase 5b wallet path planned or explicitly waived for regtest-only vault demos.
