use bitcoin::consensus::encode::{deserialize, serialize_hex};
use bitcoin::Transaction;
use phechan_bitcoin::{BitcoindRpc, Network, RpcConfig};
use phechan_validation::{validate_inscription_reveal, ValidationReport};

use crate::args::{flag_value, has_flag};
use crate::mainnet_gate::require_mainnet_broadcast_gate;

pub fn dispatch(args: &[String]) -> Result<(), String> {
    match args.get(1).map(String::as_str) {
        Some("preview") => preview(&args[2..]),
        Some("validate") => validate(&args[2..]),
        Some("broadcast") => broadcast(&args[2..]),
        Some(other) => Err(format!(
            "unknown tx subcommand '{other}' (preview|validate|broadcast)"
        )),
        None => Err("usage: phechan tx <preview|validate|broadcast>".into()),
    }
}

fn load_tx(args: &[String]) -> Result<Transaction, String> {
    let hex = flag_value(args, "--hex").ok_or("--hex <rawtx> required")?;
    let bytes = hex::decode(hex.trim()).map_err(|e| e.to_string())?;
    deserialize::<Transaction>(&bytes).map_err(|e| e.to_string())
}

fn preview(args: &[String]) -> Result<(), String> {
    let tx = load_tx(args)?;
    println!("txid: {}", tx.compute_txid());
    println!("vsize: {}", tx.vsize());
    println!("inputs: {}", tx.input.len());
    println!("outputs: {}", tx.output.len());
    println!("hex: {}", serialize_hex(&tx));
    Ok(())
}

fn validate(args: &[String]) -> Result<(), String> {
    let tx = load_tx(args)?;
    let body = flag_value(args, "--expect-body").unwrap_or_default();
    let report = if body.is_empty() {
        let mut r = ValidationReport::default();
        r.consensus_ok = Some(!tx.input.is_empty() && !tx.output.is_empty());
        r.relay_ok = Some(true);
        r.ordinals_ok = Some(true);
        r.runes_ok = Some(true);
        if tx.input.is_empty() || tx.output.is_empty() {
            r.errors.push("tx missing inputs or outputs".into());
            r.consensus_ok = Some(false);
        }
        r.warnings
            .push("no --expect-body: ordinals envelope not checked".into());
        r
    } else {
        validate_inscription_reveal(&tx, body.as_bytes())
    };
    println!("allows_broadcast: {}", report.allows_broadcast());
    for w in &report.warnings {
        println!("warning: {w}");
    }
    for e in &report.errors {
        println!("error: {e}");
    }
    Ok(())
}

fn broadcast(args: &[String]) -> Result<(), String> {
    let network =
        Network::parse(&flag_value(args, "--network").unwrap_or_else(|| "regtest".into()))
            .ok_or("invalid --network")?;
    let tx = load_tx(args)?;
    let body = flag_value(args, "--expect-body");
    let report = if let Some(b) = body {
        validate_inscription_reveal(&tx, b.as_bytes())
    } else if has_flag(args, "--skip-ordinals-check") {
        let mut r = ValidationReport::default();
        r.consensus_ok = Some(true);
        r.relay_ok = Some(true);
        r.ordinals_ok = Some(true);
        r.runes_ok = Some(true);
        r
    } else {
        return Err("provide --expect-body <text> or --skip-ordinals-check".into());
    };
    if !report.allows_broadcast() {
        return Err(format!(
            "validation blocked broadcast: {}",
            report.errors.join("; ")
        ));
    }
    require_mainnet_broadcast_gate(args, network)?;

    let rpc = BitcoindRpc::new(RpcConfig::from_env());
    let hex = serialize_hex(&tx);
    let txid = rpc.send_raw_transaction(&hex).map_err(|e| e.to_string())?;
    println!("broadcast_txid: {txid}");
    Ok(())
}
