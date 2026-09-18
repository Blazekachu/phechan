# Phechan Phase-0 Design Spec

Date: 2026-09-17  
Status: Awaiting founder review of written docs

## Summary

Phechan is a CLI-first Bitcoin **inscription** toolkit with a local UI later. Hybrid architecture: Rust protocol engine, TypeScript local UI. Sibling repo to `runes-etch` (reference only). v1 = inscriptions + UTXO asset awareness + parent preserve. Not a Runes etch product.

## Locked decisions

See `docs/architecture.md`. Key points: modular monorepo; parent default first-in/first-out with opt-in layouts; signing = keystore + PSBT; networks selectable; mainnet broadcast dual-gated.

## Spec documents

| Doc | Path |
|---|---|
| Protocol research | `docs/protocol-research.md` |
| Architecture | `docs/architecture.md` |
| Threat model | `docs/threat-model.md` |
| Signing model | `docs/signing-model.md` |
| Runes boundary | `docs/runes-integration.md` |
| Roadmap | `docs/implementation-roadmap.md` |

## Non-goals (Phase 0–2)

- Full parent-child launchpad  
- Runes etching commit-wait-reveal product flow  
- Unsafe mainnet broadcast  
- Public exposure of local UI  

## Approval

Founder approved design sections in chat (layout, core flow revised, UTXO safety, signing, runes boundary, roadmap). This file consolidates that approval for implementation planning.

**Next:** Founder reviews these files. On approval, write an implementation plan (writing-plans) for Phase 1 only.
