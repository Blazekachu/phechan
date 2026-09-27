# Studio Libraries (regtest only)

Phechan page that embeds a regtest copy of OCM Studio.

- Served at `/studio-regtest/` (UI tab **Studio Libraries** — only when wallet is connected on **regtest**).
- Recursion IDs: local ord libs only (`ocmDimensions`, `p5js`, `fflateCompress`, `tonejs`) — locked to this regtest chain; never mainnet public-goods ids for those four.
- `tonejs` → `4204859730c4c5b4425400cbe38cc8958198af32c906a26fdcc143f2757e1e69i0` (Tone.js 14.7.77, height 619).
- Download gzip uses local `fflate.esm.js` (on-chain regtest fflate is gunzip-only).
- Vite proxies `/content` + `/r` → `http://127.0.0.1:8081`.
- Download builds OCM gzip + recursion shell with those regtest IDs (not mainnet).

Do not edit the mainnet OCM Studio on `:3336` for this path.
