# Anti-sniping binding (Phase 6 design)

Status: design — implemented soft binding via vault reservations + sealed template.

## Threat

A minter (or observer) races the coordinator after seeing a promising batch allocation, replacing funding UTXOs or rewriting outputs so another party captures the child sat / parent adjacency.

## Binding layers (Phechan)

1. **Reservation** — `funding_outpoint` unique per batch; double-reserve rejected.  
2. **Seal** — `OutputTemplate` frozen at `ASSEMBLED`; ACP folklore does not allow output edits after minter `ALL|ACP` (see `docs/signing-model.md`).  
3. **Sig collect** — only signatures over sealed template accepted (product must verify sighash + outputs on import).  
4. **Parent return validator** — seal and pre-broadcast re-run sat-flow; fail closed.  
5. **Timeout / abort** — incomplete batches abort to `READY` without spending parent.

## Not claimed (yet)

- Cryptographic commit-reveal of mint intent beyond PSBT  
- Mempool replacement immunity  
- Cross-batch global rate limits  
- Public auction fairness

## Next hardening (optional)

- Hash `OutputTemplate` into an OP_RETURN / inscription metadata tag for public audit  
- MuSig2 vault co-sign so coordinator alone cannot broadcast alternate outs  
- Per-minter `SINGLE|ACP` only if product explicitly needs paired outs (not default)
