# Phechan Threat Model

Date: 2026-09-17

## 1. Assets to protect

- BTC value in user UTXOs  
- Inscription sats (parents, children, reinscriptions)  
- Runes balances co-located on UTXOs  
- Parent vault custody (V2)  
- Private keys / seed material (keystore & wallet)  
- User intent integrity (what they thought they signed)

## 2. Adversaries

| Adversary | Goal |
|---|---|
| External chain observer | Front-run, snipe mints, replace txs |
| Malicious coordinator (V2) | Redirect parent, steal fees, swap child outputs |
| Malicious minter (V2) | Steal another minter’s allocation |
| Compromised dependency | Key theft, crafted tx |
| Confused user / UI lie | Sign harmful PSBT |
| Indexer lie / lag | Wrong asset labels → accidental burn |

## 3. Trust boundaries

1. **Bitcoin consensus** — absolute for validity; not for ordinals/runes meaning.  
2. **Relay policy** — may drop valid txs.  
3. **Ordinals interpretation (`ord`)** — parent/child, satpoints.  
4. **Runes interpretation (`ord`)** — balances, cenotaphs.  
5. **Phechan validation** — must recompute from serialized tx.  
6. **Indexer APIs** — untrusted; degrade to `unknown` + hard warnings.  
7. **Wallet signing** — untrusted metadata; verify after sign.  
8. **Coordinator (V2)** — partially trusted until on-chain binding proven.

## 4. Explicit non-assumptions

Never assume:

- Signature valid without verification  
- PSBT metadata truthful  
- Request ID = ownership  
- Non-RBF ⇒ irreplaceable  
- Broadcast ⇒ confirmation in block N  
- Parent preserved without sat-flow simulation  
- Runes safe because inscription parent returned  
- “Tx confirmed” ⇒ assets safe (must check indexer outcomes)

## 5. v1 controls

| Risk | Control |
|---|---|
| Accidental mainnet broadcast | Env unlock + typed `BROADCAST MAINNET` + pass validation |
| Fee-tail parent loss | Default FI/FO template + sat-flow gate; opt-in layouts only if sim passes |
| Silent rune burn | Asset disclosure; block silent burns |
| Spending rare assets as fees | Prefer plain UTXOs; confirm asset-bearing fee inputs |
| Key leak via UI | No key export APIs; keystore regtest-oriented |
| Dependency confusion | Pin Bitcoin libs; review upgrades |

## 6. V2 (parent vault / batch) threats

| Risk | Notes |
|---|---|
| Concurrent parent spends | Reservation + single-flight lock + chain watch |
| Malicious output substitution | Sighash choice must commit critical outputs or use pre-fixed templates |
| Mint allocation sniping | Off-chain IDs insufficient; need cryptographic binding + coordinator checks; investigate on-chain enforceability |
| Mempool replacement | Watch RBF; re-validate parent return |
| Reorg | Reorg handler; parent UTXO truth from chain |

## 7. Residual risks (accepted for now)

- Indexer downtime → incomplete labels (`unknown`)  
- Wallet sighash feature gaps  
- User explicitly accepts dangerous burn (if ever enabled)  
- Soft trust in coordinator until signing spike selects a model  

## 8. Incident class already observed nearby

`runes-etch` documented a parent sat falling into the fee tail when parent was not ordered ahead of commit sats. Phechan treats **tx-confirmed ≠ parent-safe** as a first-class lesson: validate serialized assignment, then verify post-confirm with indexer when available.
