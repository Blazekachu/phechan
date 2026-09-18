# Phechan Runes Integration

Date: 2026-09-17

## 1. Boundary

| Concern | Owner |
|---|---|
| Runes etch product (named commitment, terms UI, modes) | `runes-etch` |
| Inscription tool | **Phechan** |
| Detect/disclose runes on UTXOs | **Phechan** |
| Validate co-spend so runes aren’t silently burned | **Phechan** (`phechan-runes` + validation) |
| Full etch/mint/transfer CLI product | Future / out of v1 |

## 2. Protocol-native vs application-level

| Concept | Native? |
|---|---|
| Runestone `OP_RETURN OP_13` | Protocol (`ord`) |
| Etching fields (name, spacers, symbol, divisibility, premine, terms) | Protocol |
| Mint / edicts / pointer | Protocol |
| Cenotaph behavior | Protocol |
| Ordinals parent tag `3` + parent spend | Protocol (Ordinals)—**different** from Runes |
| UI “Parent Child / Rune With Inscription / Rune” modes | Application (`runes-etch`) |
| Collection membership marketing | Application unless parent tag used |

## 3. Research checklist (from founder brief)

| Topic | Phechan stance |
|---|---|
| A. Etch + inscription content | Documented; product in runes-etch |
| B. Etch + parent-child | Documented; parent sat + rune allocation both required |
| C. Mint + inscription outs | Later; validate if touched |
| D. Transfers + parent preservation | Sat-flow ≠ rune pointer; both needed |
| E. Multiple minter funding inputs | V2; runes allocation across outs |
| F. Runes in batched txs | V2 research |
| G. OP_RETURN constraints | Single runestone; push-only payload |
| H. Cenotaph risks | Treat as fund-loss class for runes |
| I. Interaction with ordinal sat allocation | Independent rules; same tx |
| J. Multiple protocol actions in one tx | Allowed but high risk; validate all |

## 4. UTXO awareness UX (v1 requirement)

When listing/selecting UTXOs:

1. Query indexer (`ord`) when configured.  
2. Label: plain / inscription / rune / inscription+rune / unknown.  
3. Disclose amounts and IDs.  
4. If indexer missing: `unknown` + strong warning that spend may burn assets.  
5. Block paths that simulation shows burn runes unless an explicit dangerous override is added later (default: block).

## 5. Configuration model (documentation only for etch)

Do **not** invent a protocol “etch mode.” Represent etchings as field sets:

- `rune_name`, `spacers`, `symbol`, `divisibility`  
- `premine`  
- `terms`: `amount`, `cap`, `height_start`, `height_end`, `offset_start`, `offset_end`  
- `turbo`  

Validate against `ord` rules when/if etching enters Phechan.

## 6. Lessons from runes-etch (reference)

- Three **application** modes, not consensus modes.  
- Parent first-in / first-out fixed a fee-tail loss bug; other layouts can work if sat-flow is correct.  
- Name availability and 6-conf commitment matter for **named etch**—out of Phechan v1 product scope.  
- Client-side `bitcoinjs-lib` etch stack stays upstream; Phechan Rust engine should not vendor that UI.

## 7. Implementation rule

If a combined inscription+runes action is not safely validatable, **split transactions** rather than ship a hopeful combined tx.
