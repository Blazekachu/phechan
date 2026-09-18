# Windows dev setup (Phechan)

## Linker / C compiler

`rust-bitcoin` pulls in `secp256k1-sys`, which needs a C compiler.

### Option A — MSVC (recommended long-term)

1. Install [Build Tools for Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **Desktop development with C++** workload.
2. Change `rust-toolchain.toml` to `stable-x86_64-pc-windows-msvc` (or remove the file and use the default).
3. Open a fresh Developer / normal shell and run `cargo test`.

### Option B — MinGW-w64 (used during Phase 1 on this machine)

1. Install a full MinGW (not the incomplete rustup self-contained gcc). Example: WinLibs `mingw64` under `F:\Users\akhil\Main\tools\mingw64`.
2. Keep `rust-toolchain.toml` on `stable-x86_64-pc-windows-gnu`.
3. Put MinGW `bin` on `PATH` before building:

```powershell
$env:Path = "F:\Users\akhil\Main\tools\mingw64\bin;" + $env:Path
cd F:\Users\akhil\Main\phechan
cargo test
```

### Verify

```powershell
cargo test -p phechan-ordinals
cargo test -p phechan-bitcoin
cargo run -p phechan-cli -- inscription create --body "Hello, world!" --network regtest --dry-run
```
