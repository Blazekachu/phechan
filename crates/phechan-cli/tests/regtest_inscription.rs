//! Live regtest e2e — ignored by default; run with `--ignored` when bitcoind is up.
//!
//! ```text
//! cargo test -p phechan-cli --test regtest_inscription -- --ignored --nocapture
//! ```
//!
//! Requires unlocked/plain wallet RPC at `PHECHAN_RPC_URL`
//! (default `http://127.0.0.1:18444/wallet/phechan_plain`).

#[test]
#[ignore = "requires live bitcoind + phechan_plain wallet"]
fn regtest_rpc_reachable() {
    use phechan_bitcoin::{BitcoindRpc, RpcConfig};
    let rpc = BitcoindRpc::new(RpcConfig::from_env());
    let height = rpc.get_block_count().expect("rpc getblockcount");
    assert!(height > 0, "expected mined regtest chain");
}
