# Studio Libraries (regtest only)

Phechan page that embeds a regtest copy of OCM Studio.

- Served at `/studio-regtest/` (UI tab **Studio Libraries** — only when wallet is connected on **regtest**).
- Recursion IDs: local ord libs (`ocmDimensions`, `p5js`, `fflateCompress`) — see `engine/regtest-test/libs/regtest-ids.json`.
- Download gzip uses local `fflate.esm.js` (on-chain regtest fflate is gunzip-only).
- Vite proxies `/content` + `/r` → `http://127.0.0.1:8081`.
- Download builds OCM gzip + recursion shell with those regtest IDs (not mainnet).

Do not edit the mainnet OCM Studio on `:3336` for this path.
