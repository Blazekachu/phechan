# Phase 4 Local UI (Read-Only First)

> **For agentic workers:** Use executing-plans / implement task-by-task.

**Goal:** Ship a local-only web UI that previews inscriptions, UTXOs, sat-flow, and parent layouts by calling the Phechan CLI — no private keys over HTTP.

**Architecture:** Vite + React UI + tiny Node API bound to `127.0.0.1` only. API shells out to `phechan` (or `cargo run -p phechan-cli`). No wallet key APIs.

**Tech Stack:** TypeScript, Vite, React, Node http on 127.0.0.1:8787 (UI :5173 proxied).

**Spec:** `docs/architecture.md` §7, `docs/implementation-roadmap.md` Phase 4.

## Global Constraints

- Bind `127.0.0.1` only (never `0.0.0.0`).
- No endpoints that accept or return private keys / seeds / passphrases.
- Prefer dry-run / preview; live `--broadcast` behind explicit UI confirm + regtest network only.
- Share engine via CLI — do not reimplement envelopes in JS.

## Tasks

1. Scaffold `apps/local-ui` (Vite React TS)
2. Local API server (`server/index.ts`) — 127.0.0.1, CLI wrapper
3. UI: Inscription preview, UTXO list, Sat select, Parent/child dry-run
4. README run instructions + roadmap checkboxes
5. Smoke: `npm run dev` loads and dry-run returns JSON

## Exit

Founder can open `http://127.0.0.1:5173`, run inscription dry-run and see validation/disclosure without exposing keys.
