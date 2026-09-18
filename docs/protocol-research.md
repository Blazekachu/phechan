# Phechan Protocol Research

Status: Phase 0 research (2026-09-17)  
Target reference: [docs.ordinals.com](https://docs.ordinals.com/) and [`ord`](https://github.com/ordinals/ord) (normative for Runes)  
Related reference project (read-only): `F:\Users\akhil\Main\runes-etch`

This document records protocol assumptions. Each important claim includes description, source, implementation implications, testing requirements, and known limitations.

---

## 1. Scope of Phechan vs protocol surfaces

| Surface | Protocol-native? | Phechan v1 |
|---|---|---|
| Inscription envelopes / tags | Yes (Ordinals) | In scope |
| Parent spend + tag `3` provenance | Yes (Ordinals) | In scope |
| Delegate tag `11` | Yes (Ordinals) | Planned after core |
| Metadata tag `5` (CBOR) | Yes (Ordinals) | Planned |
| Metaprotocol tag `7` | Yes (field exists; semantics app-defined) | Planned with disclosure |
| Taproot output → reveal spend for inscriptions | Yes (Taproot constraint) | In scope (no forced wait) |
| Runes named-etch 6-confirmation name commitment | Yes (Runes) | **Out of product scope** (runes-etch) |
| Runes balances on UTXOs | Yes (Runes) | **Awareness / warn / validate co-spend** |
| Product “etch modes” | Application-level | Do not treat as protocol |

---

## 2. Bitcoin Core fundamentals

### 2.1 Transaction structure

- **Description:** A transaction has version, inputs (outpoint + scriptSig/witness + sequence), outputs (value + scriptPubKey), locktime. SegWit/Taproot move signatures to witness; TXID excludes witness; WTXID includes it.
- **Source:** Bitcoin developer reference / BIPs (BIP141, BIP341).
- **Implementation implications:** Vanity grinding of TXID must only mutate pre-sign fields that affect TXID; never mutate signed fields after signing.
- **Testing:** Serialize/deserialize round-trips; TXID vs WTXID fixtures.
- **Limitations:** Library version differences in PSBT field handling.

### 2.2 Fees, dust, standardness

- **Description:** Fee = sum(inputs) − sum(outputs). Dust thresholds depend on output script type and relay policy. Consensus-valid txs can still be non-standard and fail relay.
- **Source:** Bitcoin Core policy (`IsDust`, min relay fee).
- **Implementation implications:** Phechan must separate **consensus**, **relay/standardness**, and **metaprotocol** validation. Padding values like 329 or 545 sats are **examples**, not universally safe.
- **Testing:** Per-script-type dust vectors; policy vs consensus labeling in error messages.
- **Limitations:** Relay policy differs by node version and mempool settings.

### 2.3 RBF, sequence, locktime

- **Description:** Signaling replaceability does not make replacement impossible in all topologies; non-RBF is not a security boundary. Locktime can be used for TXID grinding if sequences allow.
- **Source:** BIP125; Core mempool behavior.
- **Implementation implications:** Parent vault and batch logic must handle replacement, eviction, and reorg—not assume “broadcast once = done.”
- **Testing:** RBF and non-RBF replacement scenarios on regtest.
- **Limitations:** Miner/policy variance.

### 2.4 PSBT and sighash

- **Description:** PSBT carries incomplete tx + metadata for signing. Sighash flags control what is committed: `SIGHASH_ALL`, `SINGLE`, `NONE`, and `ANYONECANPAY` combinations.
- **Source:** BIP174, BIP370; BIP143/BIP341 signature hashes.
- **Implementation implications:** `ANYONECANPAY` does **not** authorize arbitrary output changes. `SINGLE|ACP` commits one input to one matching output index—fragile under reordering.
- **Testing:** Vectors for each candidate flag under added inputs/outputs (see `signing-model.md`).
- **Limitations:** Wallet implementations differ in which sighash flags they expose.

### 2.5 Taproot signing

- **Description:** Key-path and script-path spends; inscription envelopes live in tapscript leaves revealed on spend.
- **Source:** BIP341/BIP342; Ordinals handbook inscriptions section.
- **Implementation implications:** Inscription content is in the reveal witness, not OP_RETURN. Commit output must match the script tree that will be revealed.
- **Testing:** Control block / leaf version fixtures; script-path finalize.
- **Limitations:** ECC backend choices (e.g. secp256k1) affect tooling, not consensus.

---

## 3. Ordinals / Inscriptions

### 3.1 Envelope serialization

- **Description:** Content is pushed inside `OP_FALSE OP_IF … OP_ENDIF` (“envelope”), typically after a tapscript `<pubkey> OP_CHECKSIG`. Header push `"ord"`, then tag/value pushes, empty push for body, then body chunks ≤520 bytes each.
- **Source:** [Inscriptions](https://docs.ordinals.com/inscriptions.html)
- **Implementation implications:** Build envelopes with minimal-push rules; do not data-push the envelope opcodes when composing tapscript.
- **Testing:** Golden scripts for text, multi-chunk body, tags present/absent.
- **Limitations:** Unrecognized even tags unbind; odd tags ignored—“it's okay to be odd.”

### 3.2 Defined tags (partial)

| Tag | Name | Notes |
|---|---|---|
| 1 | content_type | MIME |
| 2 | pointer | sat offset assignment |
| 3 | parent | binary inscription id; parent must be spent |
| 5 | metadata | CBOR; may split across multiple tag-5 pushes |
| 7 | metaprotocol | string identifier; semantics not universal |
| 9 | content_encoding | e.g. compression hint for *indexers/clients* |
| 11 | delegate | binary inscription id; content may 404 until delegate exists |
| 13 | rune | serialized rune (etching inscription linkage) |

- **Source:** docs.ordinals.com inscriptions field table.
- **Implementation implications:** Metaprotocol/metadata are not a global schema. Compression via tag 9 is a **content encoding hint**, not proof that all clients decompress (Brotli is experimental unless verified against target `ord`/clients).
- **Testing:** Tag round-trips; unbound even-tag cases; delegate without body.
- **Limitations:** Display of exotic CBOR may fail in `ord`.

### 3.3 Inscription IDs and sat assignment

- **Description:** ID = `reveal_txid + "i" + envelope_index`. Without pointer, inscription is on the first sat of its input. Pointer can select another sat.
- **Source:** docs.ordinals.com inscriptions.
- **Implementation implications:** Sat-flow simulation must model pointer and input ordering.
- **Testing:** Multi-envelope reveals; pointer placement.
- **Limitations:** Historical cursed/jubilee numbering quirks for explorers—not required for creation safety.

### 3.4 Parent / child (provenance)

- **Description:** Child includes tag `3` with parent id (32-byte TXID little-endian order as used by ord + LE index with trailing zeros omitted). Parent inscription’s UTXO must be spent in the reveal (inscribe) transaction. Multiple parents allowed (multiple tag 3). Burning parent closes collection issuance.
- **Source:** [Provenance](https://docs.ordinals.com/inscriptions/provenance.html); ord discussions.
- **Implementation implications:** Parent **position** is not fixed by protocol. Safety = ordinal sat assignment of the parent’s sat into the intended return output. Default Phechan template: first input / first output. Opt-in: e.g. last input → first or last output, if simulation passes.
- **Testing:** Layout matrix; fee-tail loss regression (known class of bug in etch tools).
- **Limitations:** Parent UTXO may also carry runes; ordinals parent ≠ runes parent.

### 3.5 Reinscription / delegate

- **Description:** Reinscription appends another inscription to an already-inscribed sat. Delegate tag 11 serves another inscription’s content.
- **Source:** docs.ordinals.com reinscription / delegate.
- **Implementation implications:** Selection must verify ownership and satpoint; reinscribe is not overwrite.
- **Testing:** Satpoint-targeted reinscribe; delegate 404 until target exists.
- **Limitations:** Indexer must support sat index for some sat-number workflows.

### 3.6 “Commit / reveal” wording (critical)

- **Description:** Creating a Taproot output that commits to inscription tapscript, then spending it, is required by Taproot mechanics for standard envelopes. That is **not** the same as Runes’ **six-confirmation name commitment** for non-reserved rune names.
- **Source:** Ordinals inscriptions; Runes specification / `ord`.
- **Implementation implications:** Phechan inscription flows use create+spend as needed **without** forced multi-confirmation wait. Runes name-commitment wait is **out of product scope** (handled by runes-etch).
- **Testing:** Back-to-back create+reveal on regtest for inscriptions.
- **Limitations:** Wallets may batch or coinjoin funding differently; engine must not assume a single fixed template from runes-etch.

---

## 4. Runes (awareness for co-spend; etching product out of scope)

### 4.1 Normative rule

- **Description:** Prose is a guide; **`ord` is the specification**. Alternative implementations risk cenotaphs and incorrect balances.
- **Source:** [Runes specification page](https://docs.ordinals.com/runes/specification.html) (“Runes Does Not Have a Specification”).
- **Implementation implications:** Prefer golden vectors from `ord` tests; never invent etch UX “modes” as protocol.
- **Testing:** Decode/encode parity against `ord` fixtures when crate is enabled.
- **Limitations:** Phechan v1 does not ship etching product features.

### 4.2 Runestone basics

- **Description:** At most one runestone per tx. Output script starts `OP_RETURN OP_13` then data pushes → LEB128 u128 sequence → tags/edicts. Etching, mint, edicts, pointer. Malformed → cenotaph (burns input runes; etching unmintable; mint counts toward cap but minted burned).
- **Source:** docs.ordinals.com runes + specification.
- **Implementation implications:** Co-spending a rune UTXO without a correct runestone (or with a cenotaph) can burn runes. Asset disclosure must warn.
- **Testing:** Cenotaph cases; pointer out of range; edict output bounds.
- **Limitations:** Only one runestone; cannot assume arbitrary combination with other OP_RETURN protocols.

### 4.3 Etching fields (for documentation / future validation)

Configuration model (not “etch mode”):

- name, spacers, symbol, divisibility  
- premine  
- terms: amount, cap, height start/end, offset start/end  
- turbo flag  

Named non-reserved etchings require a name commitment in an aged (≥6 conf) input tapscript—**product in runes-etch, not Phechan v1**.

### 4.4 Coexistence with inscriptions

- **Description:** A reveal may include inscription envelopes **and** a runestone output. Tag `13` can link an inscription to a rune. Parent inscription spend is Ordinals-specific; Runes allocation follows edicts/pointer independently.
- **Source:** Ordinals/Runes docs; runes-etch production experience (parent first-in/first-out after fee-tail incident).
- **Implementation implications:** Validate **both** sat-flow and rune allocation when both assets exist on inputs. Do not assume “using a parent” means the same in both protocols.
- **Testing:** Parent+rune UTXO preserve; inscription-only; rune-only fee input refusal.
- **Limitations:** Combined multi-protocol txs increase cenotaph and fee-tail risk.

---

## 5. Libraries (language decision)

| Stack | Strengths | Gaps |
|---|---|---|
| **Rust `rust-bitcoin` / BDK** | PSBT, Taproot, auditability, CLI | Ordinals/Runes not built-in |
| **TS `bitcoinjs-lib` 7** | Proven in runes-etch; fast UI | Pinning/`__CACHE` hazards; weaker for long-term core |
| **`ord` binary** | Normative Runes/inscriptions indexing | Not an embeddable library API |

**Decision:** Hybrid — Rust crates for engine; TypeScript for local UI. See `architecture.md`.

---

## 6. Confirmed vs uncertain

### Confirmed

- Inscription envelopes and parent tag semantics.  
- Parent safety is sat-assignment, not a fixed index law.  
- Runes `ord`-normative; cenotaph burns.  
- ACP ≠ free output mutation.  
- Named rune 6-conf commitment ≠ general inscription requirement.

### Uncertain (must resolve before mainnet / V2)

- Exact sighash scheme for multi-minter batch (needs regtest spikes).  
- Whether on-chain allocation binding can fully prevent mint sniping without coordinator trust.  
- Indexer lag / reorg UX thresholds for “unknown” asset labels.  
- Brotli/`content_encoding` client support matrix for target `ord` version.  
- Practical TXID vanity difficulty vs fee/locktime constraints for inscription-only txs.

---

## 7. Sources

- https://docs.ordinals.com/inscriptions.html  
- https://docs.ordinals.com/inscriptions/provenance.html  
- https://docs.ordinals.com/inscriptions/delegate.html  
- https://docs.ordinals.com/inscriptions/metadata.html  
- https://docs.ordinals.com/runes.html  
- https://docs.ordinals.com/runes/specification.html  
- https://github.com/ordinals/ord  
- Local lessons: `runes-etch` README + SECURITY.md (parent fee-tail bug class)
