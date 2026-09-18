# Phechan Implementation Roadmap

Date: 2026-09-17

## Phase 0 — Research + scaffold

- [x] Inspect workspace / runes-etch reference  
- [x] Architecture decisions with founder  
- [x] Docs: protocol, architecture, threat, signing, runes, roadmap  
- [x] Minimal Cargo workspace + CLI stub with mainnet dual-gate  
- [x] No launchpad implementation  

**Exit:** Founder accepts written specs.

## Phase 1 — Inscription core (regtest)

- [x] Envelope builder (text)  
- [x] Taproot create + reveal spend helpers  
- [x] PSBT create / sign / finalize (reveal script-path)  
- [x] Validation report (structural inscription)  
- [x] Regtest keystore sign path  
- [x] Tests: golden envelope bytes  
- [x] CLI `inscription create --dry-run`  
- [x] Live bitcoind e2e (`inscription create --broadcast`)  

**Exit:** Create+reveal text inscription dry-run on regtest path without parent — **met**.

## Phase 2 — Parent preserve + asset disclosure

- [x] UTXO label model + disclosure printer  
- [x] Sat-flow simulator  
- [x] Default parent FI/FO template  
- [x] Opt-in layouts behind simulation  
- [x] Rune-bearing UTXO warnings / blocks  
- [x] Regression: parent cannot enter fee tail  
- [x] CLI `inscription child --dry-run`  

**Exit:** Parent+child layout validation with serialized sat-flow — **met**. Live child broadcast lives under Phase 3.

## Phase 3 — CLI surface (inscription-first)

- [x] Modular CLI: `inscription`, `utxo`, `sat`, `psbt`, `tx`  
- [x] `inscription create --broadcast` live regtest path  
- [x] `inscription inspect`, `utxo list|inspect`, `sat select`, `psbt inspect`  
- [x] `tx preview|validate|broadcast` with mainnet dual gate  
- [x] Bitcoind JSON-RPC helper (`phechan-bitcoin::rpc`)  
- [x] `inscription delegate` (tag 11) dry-run + regtest broadcast  
- [x] `inscription reinscribe` (satpoint + append disclosure) dry-run + regtest broadcast  
- [x] `inscription child --broadcast` FI/FO live path (`--parent-outpoint`, wallet+keystore PSBT)  

Only implement when underlying behavior is tested.  
Broadcast gates enforced.

**Exit:** Phase 3 leftovers closed — **met**.

## Phase 4 — Local UI (read-only first)

- [x] `apps/local-ui` Vite + React  
- [x] API + UI bound to `127.0.0.1` only  
- [x] CLI-backed dry-run: inscribe, parent/child, utxo list, sat select  
- [x] Reject private-key / seed payloads  
- [x] Live broadcast from UI (typed `BROADCAST REGTEST`; regtest-only)  
- [x] Ord-labeled UTXOs (`utxo list --ord` + UI checkbox; `PHECHAN_ORD_URL`)  

**Exit:** Local UI dry-run + gated regtest broadcast + ord labeling path — **met**.

## Phase 5 — Signing spikes (V2 prep)

- [x] Regtest matrix for ACP candidates (`phechan-psbt` `spike_*` tests)  
- [x] Fill guarantees in `signing-model.md`  
- [x] Parent vault state machine design under `protocols/parent-vault/STATE_MACHINE.md`  

**Exit:** Founder can read what each sighash guarantees; vault states designed on paper; ACP output-edit folklore disproved — **met**.

## Phase 5b — Wallet connect + signet

- [x] Wallet connect path (Xverse / BitcoinProvider in local UI)  
- [x] **Xverse** PSBT sign + paste-import finalize (`psbt finalize-import`)  
- [x] Same final-tx revalidation as keystore path  
- [x] Signet selectable (`--network signet`, UI confirm `BROADCAST SIGNET`)  
- [x] `--unsigned-psbt` export for wallet signing  

**Exit:** PSBT wallet path + signet target wired — **met** (live Xverse soak still operator-run).

Keep v1 inscription tool on keystore **and** wallet PSBT.

## Phase 6 — Batch launchpad (V2)

- [x] Mint requests / reservation (`phechan-vault` + `phechan vault reserve`)  
- [x] Batching state machine (fund → open → seal → sigs → broadcast → settle)  
- [x] Parent vault return validator at seal (`validate_parent_child_layout`)  
- [x] Anti-sniping binding design (`protocols/parent-vault/ANTI_SNIPING.md`)  
- [x] Phase 5 selection honored (`ALL|ACP` docs; sealed template)  
- [x] Live wallet path available from 5b before non-regtest vault demo  

**Exit:** Vault states driveable via CLI on disk; parent return enforced at seal — **met** (demo / regtest; no production parent).

## Phase 7 — Deepening

- [x] Metadata / metaprotocol validation docs (`docs/metadata-metaprotocol.md`) + envelope tags 5/7/9  
- [x] Compression experiments notes (`docs/content-encoding.md`; default stays uncompressed)  
- [x] Vanity grinder module (`phechan-bitcoin::vanity` — locktime grind; post-sign mutate list)  
- [x] Runes co-spend encode helpers (`phechan-runes::cospend` — marker/advice; **not** etch product)  

**Exit:** Phase 7 deepening landed — **met**.

## Mainnet readiness gate (hard)

Do not claim mainnet-ready until:

1. Extensive regtest coverage including parent/rune cases  
2. Signet soak with real `ord` (and wallet path from Phase 5b)  
3. Signing model doc completed for any multi-party features used  
4. Security review of broadcast + keystore paths  
5. Dual mainnet gate remains on  

## First working prototype (summary)

**Inscription text create/spend on regtest + parent preserve round-trip with asset disclosure.**  
Default parent FI/FO; optional layouts if sat-flow passes.  
No Runes etch ceremony. No launchpad.
