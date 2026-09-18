# Phechan Architecture

Date: 2026-09-17  
Status: Approved design direction (founder review of written specs still required)

## 1. Product positioning

**Phechan** — CLI-first Bitcoin inscription and transaction construction toolkit with a local UI on the same engine.

Tagline: *Your Bitcoin. Your Sats. Your Protocol.*

**v1 focus:** Inscription tool + UTXO/asset awareness (inscriptions, runes, unknown) + parent preserve.  
**Not v1:** Runes etching product (exists as `runes-etch`), public servers, full collection launchpad.

## 2. Decisions locked

| Decision | Choice |
|---|---|
| Repo location | `F:\Users\akhil\Main\phechan` (sibling to runes-etch) |
| Relation to runes-etch | Reference only; do not modify |
| Core language | Hybrid: Rust engine + TS local UI |
| Monorepo shape | Modular Cargo workspace + `apps/local-ui` |
| First prototype | Inscription create/spend + parent preserve |
| Parent placement | Default first-in/first-out; opt-in layouts if simulation passes |
| Signing | Regtest keystore **and** PSBT export/import |
| Networks | All selectable for build/preview |
| Mainnet broadcast | Dual gate: `PHECHAN_ALLOW_MAINNET_BROADCAST=1` **and** typed `BROADCAST MAINNET` + passing validation |

## 3. Repository layout

```
phechan/
├── Cargo.toml
├── apps/local-ui/
├── crates/
│   ├── phechan-cli/
│   ├── phechan-bitcoin/
│   ├── phechan-psbt/
│   ├── phechan-ordinals/
│   ├── phechan-runes/          # decode/validate co-spend; not etch product
│   ├── phechan-sat/
│   ├── phechan-validation/
│   └── phechan-keystore/       # regtest-oriented
├── protocols/parent-vault/     # V2 design first
├── docs/
├── tests/
└── examples/
```

## 4. Crate responsibilities

| Crate | Responsibility |
|---|---|
| `phechan-bitcoin` | Tx types helpers, fee calc, dust helpers, network params |
| `phechan-psbt` | Build/inspect/finalize PSBT wrappers |
| `phechan-ordinals` | Envelope, tags, reveal script tree helpers |
| `phechan-runes` | Runestone parse/validate for safety; no etch UX |
| `phechan-sat` | Sat-flow simulation, parent placement policies |
| `phechan-validation` | Layered reports: consensus / relay / ordinals / runes / assets |
| `phechan-keystore` | Local keys for regtest/CI only |
| `phechan-cli` | User-facing `phechan` commands |

**Rule:** Crates do not broadcast. Only CLI (and later a tightly scoped local server) may broadcast, and only after validation + network gates.

## 5. Data flow (v1 inscription)

```
Content → envelope → taproot script tree
                  ↓
            funding / create output
                  ↓
     reveal tx (+ optional parent input)
                  ↓
     sat-flow + asset disclosure report
                  ↓
     PSBT → sign (keystore | wallet) → finalize
                  ↓
     validate(serialized tx) → broadcast gates
```

No forced multi-block wait for plain inscriptions. Taproot create+spend is mechanical, not the Runes name-commitment ceremony.

## 6. UTXO asset model

Labels: `plain` | `inscription` | `rune` | `inscription+rune` | `unknown`

Before any spend path, CLI prints disclosure of what each selected UTXO bears. Fee selection prefers plain payment UTXOs. Selecting asset-bearing UTXOs for fees requires explicit confirmation.

## 7. Local UI

- Local-only default bind (e.g. `127.0.0.1`)
- Shares engine via CLI subprocess or local RPC wrapping the same validation
- No private key exfiltration APIs
- Read-only preview first; signing remains wallet/keystore paths

## 8. What we will not build in the engine

- Invented protocol “modes” that are only UI presets  
- Silent mainnet broadcast  
- Trust of request IDs or PSBT metadata without re-validation  
- Assuming non-RBF or “tx confirmed” means assets are safe  

## 9. Open items needing later approval

- Exact CLI command taxonomy (may rename after use)  
- V2 batch signing scheme (after spikes)  
- Whether local UI uses HTTP JSON or unix/named pipe to CLI  
- Target pinned `ord` version for golden vectors  

See also: `threat-model.md`, `signing-model.md`, `runes-integration.md`, `implementation-roadmap.md`.
