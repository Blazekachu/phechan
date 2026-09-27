# Phechan

**Your Bitcoin. Your Sats. Your Protocol.**

CLI-first Bitcoin inscription and transaction construction toolkit. Local UI later, same engine.

> Phase 3: CLI polish + live regtest inscription create/reveal. No launchpad. No Runes etch product.

## Status

- Docs under `docs/`
- Rust workspace with envelope → Taproot commit → reveal PSBT → regtest sign → validation
- Mainnet broadcast dual-gated; `tx broadcast` still blocked without a full validation path for arbitrary txs

## Quick start

Windows needs a C toolchain for `secp256k1-sys` — see [`docs/dev-setup-windows.md`](docs/dev-setup-windows.md).

```powershell
$env:Path = "F:\Users\akhil\Main\tools\mingw64\bin;" + $env:Path   # if using MinGW
cd F:\Users\akhil\Main\phechan
cargo test
cargo run -p phechan-cli -- inscription create --body "Hello, world!" --network regtest --dry-run
cargo run -p phechan-cli -- inscription child --body "child" --parent "<inscription_id>" --network regtest --dry-run
# live (needs bitcoind + unencrypted wallet phechan_plain):
cargo run -p phechan-cli -- inscription create --body "phechan-e2e" --network regtest --broadcast
```

## Local UI

```powershell
$env:Path = "F:\Users\akhil\Main\tools\mingw64\bin;" + $env:Path
cd F:\Users\akhil\Main\phechan\apps\local-ui
npm install
npm run dev
```

Open http://127.0.0.1:5173 (localhost only).

## Networks / broadcast

- Build and preview: any network  
- Broadcast: network comes from the connected wallet (header pill). No env unlock gate.

### Ord indexer (Verify parent / sat / delegate)

- **Mainnet:** public [ordinals.com](https://ordinals.com) recursive API (`/r/inscription`, `/r/sat`, `/r/utxo`) first; optional [Ordiscan](https://ordiscan.com) via `PHECHAN_ORDISCAN_API_KEY` (Ordiscan’s HTTP API requires a key); optional override `PHECHAN_ORD_URL`.
- **Signet / testnet / regtest:** `PHECHAN_ORD_URL` (default `http://127.0.0.1:8080`).
## Docs

- [Protocol research](docs/protocol-research.md)
- [Architecture](docs/architecture.md)
- [Threat model](docs/threat-model.md)
- [Signing model](docs/signing-model.md)
- [Runes integration](docs/runes-integration.md)
- [Implementation roadmap](docs/implementation-roadmap.md)
- [Phase 1 plan](docs/superpowers/plans/2026-09-17-phase1-inscription-core.md)

## License

TBD
