# Phase 3 CLI Polish + Live Regtest E2E

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans or subagent-driven-development.

**Goal:** Polish the `phechan` CLI surface and prove a live regtest text inscription create→reveal→mine path against the local stack.

**Architecture:** Minimal JSON-RPC client for bitcoind; wallet funds the Phechan-computed commit address; Phechan script-path-signs and broadcasts reveal; CLI modules for inspect/utxo/psbt/tx.

**Tech Stack:** Existing crates + `ureq` (or std HTTP) for RPC; regtest-stack on `127.0.0.1:18444`.

**Spec:** `docs/implementation-roadmap.md` Phase 3; `docs/architecture.md`.

## Global Constraints

- Broadcast gates unchanged (mainnet dual gate).
- Live broadcast only for regtest/signet/testnet with validation pass; regtest e2e uses RPC env defaults.
- No Runes etch. Parent live e2e optional if time; text create is the exit criterion.
- Default RPC: `http://127.0.0.1:18444` user `ord` pass `regtest-local-dev` (override via env).

## Tasks

1. Start/verify regtest stack (bitcoind at least)
2. `phechan-bitcoin` RPC helper (`getrawtransaction`, `sendrawtransaction`, `sendtoaddress`, `listunspent`, `generatetoaddress`/`getnewaddress`)
3. CLI modules + commands: `inscription inspect`, `utxo list|inspect`, `psbt inspect`, `tx preview|validate|broadcast`
4. `inscription create --broadcast` live path (wallet→commit address→reveal→mine)
5. Enable/un-ignore e2e test or `examples/regtest_e2e.md` + cargo test ignored harness calling RPC
6. Update roadmap

## Exit

`phechan inscription create --body "phechan-e2e" --network regtest --broadcast` mines a reveal; `bitcoin-cli getrawtransaction` sees it.
