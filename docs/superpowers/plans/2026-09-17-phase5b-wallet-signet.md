# Phase 5b — Wallet connect + Xverse + signet

> **For agentic workers:** execute task-by-task.

**Goal:** PSBT wallet path (Xverse/signet-capable) with same final-tx revalidation as keystore; local UI wallet-connect for signing; signet selectable.

**Architecture:** CLI builds unsigned PSBT → export base64 → wallet signs → import → finalize → validate → broadcast (signet/regtest; mainnet dual gate). UI uses sats-connect / Xverse provider for `signPsbt` when available; falls back to paste-PSBT.

**Tech Stack:** Existing Rust CLI; local-ui + sats-connect; bitcoind signet RPC env vars.

**Spec:** `docs/signing-model.md` §5; roadmap Phase 5b.

## Global Constraints

- Never accept private keys in UI API.
- Re-validate final serialized tx after any wallet signature.
- Mainnet still dual-gated via `tx broadcast`.
- Regtest keystore path remains for CI.

## Tasks

1. CLI `psbt export-reveal` / `psbt finalize-import` helpers + signet network on create dry-run
2. Allow `--network signet` for dry-run; broadcast signet via env RPC (no mainnet)
3. Local UI: PSBT paste finalize + optional Xverse `signPsbt` (sats-connect)
4. Docs: Phase 5b checklist + signet soak notes
5. Roadmap mark 5b complete

## Exit

Founder can sign a reveal PSBT with Xverse (or paste), revalidate, and target signet when RPC is configured.
