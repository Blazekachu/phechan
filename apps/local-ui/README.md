# Phechan local UI

Inscription-first local UI. Keys stay in the wallet for funding; reveal is completed by the engine after commit is funded.

## Flow

1. **Connect wallet** — address + network
2. Choose **Text / Delegate / Upload**
3. Optional: title (Properties), metadata, metaprotocol, Brotli
4. Advanced: parent / sat targeting
5. Tx options: fee rate (sats/vB), postage, vanity, OP_RETURN
6. **Preview** — content render only  
7. **Inscribe** — prepare → build funding PSBT at your fee rate (optional commit TXID grind) → sign in wallet → reveal grind → broadcast → mempool link

**Fee rate** applies to commit funding and reveal. Pick funding UTXO under Transaction options (or Auto). Commit vanity ≠ reveal vanity.

## Run

```powershell
$env:Path = "F:\Users\akhil\Main\tools\mingw64\bin;" + $env:Path
cd F:\Users\akhil\Main\phechan\apps\local-ui
npm install
npm run dev
```

Open **http://127.0.0.1:5173**

Needs `PHECHAN_BIN` (or cargo-built `phechan` on PATH) and `PHECHAN_RPC_*` pointing at a node for the same network as the wallet (signet/testnet) so reveal can look up the commit tx and broadcast.

## Security

No private-key APIs. Mainnet inscribe blocked in the UI.
