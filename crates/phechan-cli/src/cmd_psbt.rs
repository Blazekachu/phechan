use bitcoin::consensus::encode::serialize_hex;
use bitcoin::psbt::Psbt;
use base64::Engine;
use phechan_bitcoin::{BitcoindRpc, Network, RpcConfig};
use phechan_psbt::{ensure_nested_redeem_from_pubkey, finalize_to_tx};
use phechan_validation::{validate_inscription_reveal, ValidationReport};

use crate::args::{flag_value, has_flag};
use crate::mainnet_gate::require_mainnet_broadcast_gate;

pub fn dispatch(args: &[String]) -> Result<(), String> {
    match args.get(1).map(String::as_str) {
        Some("inspect") => inspect(&args[2..]),
        Some("finalize-import") => finalize_import(&args[2..]),
        Some(other) => Err(format!(
            "unknown psbt subcommand '{other}' (inspect|finalize-import)"
        )),
        None => Err("usage: phechan psbt <inspect|finalize-import>".into()),
    }
}

fn load_psbt(args: &[String]) -> Result<Psbt, String> {
    let b64 = flag_value(args, "--base64").ok_or("--base64 <psbt> required")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| e.to_string())?;
    Psbt::deserialize(&bytes).map_err(|e| e.to_string())
}

fn inspect(args: &[String]) -> Result<(), String> {
    let network =
        Network::parse(&flag_value(args, "--network").unwrap_or_else(|| "signet".into()))
            .unwrap_or(Network::Signet);
    let btc_net = network.to_bitcoin();
    let psbt = load_psbt(args)?;
    let tx = &psbt.unsigned_tx;

    println!("network: {}", network.as_str());
    println!("inputs: {}", tx.input.len());
    println!("outputs: {}", tx.output.len());
    println!("unsigned_txid: {}", tx.compute_txid());

    let mut in_sum: u64 = 0;
    let mut in_sum_ok = true;
    for (i, txin) in tx.input.iter().enumerate() {
        let psbt_in = psbt.inputs.get(i);
        let (value, spk) = if let Some(utxo) = psbt_in.and_then(|p| p.witness_utxo.as_ref()) {
            (Some(utxo.value.to_sat()), Some(utxo.script_pubkey.clone()))
        } else if let Some(txout) = psbt_in
            .and_then(|p| p.non_witness_utxo.as_ref())
            .and_then(|prev| prev.output.get(txin.previous_output.vout as usize))
        {
            (Some(txout.value.to_sat()), Some(txout.script_pubkey.clone()))
        } else {
            in_sum_ok = false;
            (None, None)
        };
        if let Some(v) = value {
            in_sum = in_sum.saturating_add(v);
        }
        let addr = spk
            .as_ref()
            .and_then(|s| bitcoin::Address::from_script(s, btc_net).ok())
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".into());
        let val_s = value
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".into());
        println!(
            "vin[{i}]: {}:{}  value={val_s}  address={addr}",
            txin.previous_output.txid, txin.previous_output.vout
        );
    }

    let mut out_sum: u64 = 0;
    for (i, txout) in tx.output.iter().enumerate() {
        let v = txout.value.to_sat();
        out_sum = out_sum.saturating_add(v);
        let addr = bitcoin::Address::from_script(&txout.script_pubkey, btc_net)
            .map(|a| a.to_string())
            .unwrap_or_else(|_| format!("script={}", txout.script_pubkey));
        println!("vout[{i}]: value={v}  address={addr}");
    }

    if in_sum_ok && in_sum >= out_sum {
        println!("fee_sats: {}", in_sum - out_sum);
    }
    Ok(())
}

/// Import a wallet-signed PSBT, re-validate the final tx, optionally broadcast.
fn finalize_import(args: &[String]) -> Result<(), String> {
    let network =
        Network::parse(&flag_value(args, "--network").unwrap_or_else(|| "regtest".into()))
            .ok_or("invalid --network")?;

    let mut psbt = load_psbt(args)?;
    // Xverse often strips redeemScript / final_script_sig on nested payment — restore from pubkey.
    if let Some(pk) = flag_value(args, "--funding-pubkey-hex") {
        ensure_nested_redeem_from_pubkey(&mut psbt, &pk).map_err(|e| e.to_string())?;
    }
    for (i, inp) in psbt.inputs.iter().enumerate() {
        println!(
            "input[{i}]_pre_finalize: final_wit={} partial_sigs={} redeem={} script_sig_final={}",
            inp.final_script_witness.as_ref().map(|w| w.len()).unwrap_or(0),
            inp.partial_sigs.len(),
            inp.redeem_script.is_some(),
            inp.final_script_sig.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
        );
    }
    // Always use finalize_to_tx (partial_sigs → witness + nested scriptSig). Do NOT prefer
    // extract_tx — rust-bitcoin may extract P2SH spends with empty scriptSig.
    let tx = finalize_to_tx(&psbt).map_err(|e| e.to_string())?;
    for (i, inp) in tx.input.iter().enumerate() {
        println!(
            "input[{i}]_script_sig_len: {} witness_stack: {}",
            inp.script_sig.len(),
            inp.witness.len()
        );
    }

    let body = flag_value(args, "--expect-body");
    let report = if let Some(b) = &body {
        validate_inscription_reveal(&tx, b.as_bytes())
    } else if has_flag(args, "--skip-ordinals-check") {
        let mut r = ValidationReport::default();
        r.consensus_ok = Some(!tx.input.is_empty());
        r.relay_ok = Some(true);
        r.ordinals_ok = Some(true);
        r.runes_ok = Some(true);
        r.warnings
            .push("skipped ordinals envelope check (--skip-ordinals-check)".into());
        r
    } else {
        return Err("require --expect-body <text> or --skip-ordinals-check".into());
    };

    println!("txid: {}", tx.compute_txid());
    println!("hex: {}", serialize_hex(&tx));
    println!("allows_broadcast: {}", report.allows_broadcast());
    for w in &report.warnings {
        println!("warning: {w}");
    }
    for e in &report.errors {
        println!("error: {e}");
    }
    if !report.allows_broadcast() {
        return Err("validation blocked broadcast".into());
    }

    if !has_flag(args, "--broadcast") {
        println!("dry-run: not broadcast (pass --broadcast)");
        return Ok(());
    }

    require_mainnet_broadcast_gate(args, network)?;

    if network != Network::Regtest
        && network != Network::Signet
        && network != Network::Testnet
        && network != Network::Mainnet
    {
        return Err("psbt finalize-import --broadcast supports regtest|signet|testnet|mainnet".into());
    }

    let hex = serialize_hex(&tx);
    // sort-utxo / runes-etch: public Esplora first (allows <1 sat/vB). Local node
    // often rejects with minrelay ≈ 1 sat/vB.
    let txid = match network {
        Network::Regtest => {
            let rpc = BitcoindRpc::new(RpcConfig::from_env());
            rpc.send_raw_transaction(&hex).map_err(|e| e.to_string())?
        }
        _ => match phechan_bitcoin::esplora_broadcast_tx(network, &hex) {
            Ok(id) => {
                println!("broadcast_via: esplora");
                id
            }
            Err(esplora_err) => {
                // Fall back to local bitcoind (works when fee ≥ minrelay)
                let rpc = BitcoindRpc::new(RpcConfig::from_env());
                match rpc.send_raw_transaction(&hex) {
                    Ok(id) => {
                        println!("broadcast_via: bitcoind");
                        println!("note: esplora failed ({esplora_err}); used local node");
                        id
                    }
                    Err(rpc_err) => {
                        return Err(format!(
                            "broadcast failed — esplora: {esplora_err}; bitcoind: {rpc_err}"
                        ));
                    }
                }
            }
        },
    };
    println!("broadcast_txid: {txid}");
    println!("network: {}", network.as_str());
    Ok(())
}
