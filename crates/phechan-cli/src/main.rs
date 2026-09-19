//! Phechan CLI — inscription toolkit with regtest broadcast.

mod args;
mod cmd_inscription;
mod cmd_psbt;
mod cmd_sat;
mod cmd_tx;
mod cmd_utxo;
mod cmd_vault;
mod mainnet_gate;

fn print_help() {
    println!(
        "\
phechan — Your Bitcoin. Your Sats. Your Protocol.

Usage:
  phechan inscription create --body <text> | --body-file <path> | --body-hex <hex> --network regtest|signet (--dry-run|--broadcast|--unsigned-psbt)
  phechan inscription child --parent <id> --body <text> (--dry-run|--broadcast --parent-outpoint txid:vout)
  phechan inscription delegate --delegate <id> [--body <text>] (--dry-run|--broadcast)
  phechan inscription reinscribe --satpoint txid:vout --body <text> (--dry-run|--broadcast)
  phechan inscription inspect --body <text> | --envelope-hex <hex>
  phechan utxo list [--ord] | utxo inspect --txid <id> --vout <n> [--ord]
  phechan sat select --inputs a,b --outputs c,d --at vin:offset
  phechan psbt inspect --base64 <psbt> | --base64-file <path>
  phechan psbt finalize-import (--base64 <psbt> | --base64-file <path>) [--expect-body <text> | --expect-body-file <path>] [--broadcast] [--network regtest|signet]
  phechan vault status|fund|open-batch|reserve|seal|collect-sig|broadcast-ready|settle|abort|close
  phechan tx preview|validate|broadcast --hex <rawtx> [--network regtest]

RPC (regtest defaults): PHECHAN_RPC_URL PHECHAN_RPC_USER PHECHAN_RPC_PASS
  default http://127.0.0.1:18444/wallet/phechan_plain ord / regtest-local-dev
Ord labels: PHECHAN_ORD_URL (default http://127.0.0.1:80) with utxo list --ord
Vault state: PHECHAN_VAULT_ROOT (default cwd) → .phechan/vault/state.json

Mainnet broadcast: PHECHAN_ALLOW_MAINNET_BROADCAST=1 + --confirm \"BROADCAST MAINNET\"
"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let result = match args.first().map(String::as_str) {
        Some("inscription") => cmd_inscription::dispatch(&args),
        Some("utxo") => cmd_utxo::dispatch(&args),
        Some("sat") => cmd_sat::dispatch(&args),
        Some("psbt") => cmd_psbt::dispatch(&args),
        Some("tx") => cmd_tx::dispatch(&args),
        Some("vault") => cmd_vault::dispatch(&args),
        Some(other) => Err(format!("unknown command '{other}'. Try --help")),
        None => Ok(()),
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use phechan_bitcoin::Network;
    use phechan_validation::ValidationReport;

    #[test]
    fn phase0_style_report_still_blocks_empty_broadcast_path() {
        let report = ValidationReport::phase0_blocked(Network::Regtest);
        assert!(!report.allows_broadcast());
    }
}
