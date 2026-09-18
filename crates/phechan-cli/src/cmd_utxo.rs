use phechan_bitcoin::{BitcoindRpc, OrdClient, RpcConfig};
use phechan_runes::{format_disclosure, LabeledUtxo, UtxoAssetHint};

use crate::args::{flag_value, has_flag};

pub fn dispatch(args: &[String]) -> Result<(), String> {
    match args.get(1).map(String::as_str) {
        Some("list") => list(&args[2..]),
        Some("inspect") => inspect(&args[2..]),
        Some(other) => Err(format!("unknown utxo subcommand '{other}' (list|inspect)")),
        None => Err("usage: phechan utxo <list|inspect>".into()),
    }
}

fn list(args: &[String]) -> Result<(), String> {
    let use_ord = has_flag(args, "--ord") || std::env::var("PHECHAN_ORD_URL").is_ok();
    let rpc = BitcoindRpc::new(RpcConfig::from_env());
    let utxos = rpc.list_unspent().map_err(|e| e.to_string())?;
    let arr = utxos.as_array().ok_or("listunspent not array")?;
    let ord = if use_ord {
        Some(OrdClient::from_env())
    } else {
        None
    };

    let mut labeled = Vec::new();
    let mut ord_hits = 0u32;
    let mut ord_misses = 0u32;
    for u in arr {
        let txid = u.get("txid").and_then(|v| v.as_str()).unwrap_or("?");
        let vout = u.get("vout").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let amount = u.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let sats = (amount * 100_000_000.0).round() as u64;

        let (hint, inscription_ids, rune_summary) = if let Some(client) = &ord {
            let info = client.output_best_effort(txid, vout);
            if info.found {
                ord_hits += 1;
            } else {
                ord_misses += 1;
            }
            let has_i = !info.inscription_ids.is_empty();
            let has_r = info.rune_summary.is_some();
            let hint = match (has_i, has_r, info.found) {
                (true, true, _) => UtxoAssetHint::InscriptionAndRune,
                (true, false, _) => UtxoAssetHint::Inscription,
                (false, true, _) => UtxoAssetHint::Rune,
                (false, false, true) => UtxoAssetHint::Plain,
                (false, false, false) => UtxoAssetHint::Unknown,
            };
            (hint, info.inscription_ids, info.rune_summary)
        } else {
            (UtxoAssetHint::Unknown, vec![], None)
        };

        labeled.push(LabeledUtxo {
            txid_hex: txid.to_string(),
            vout,
            value: sats,
            hint,
            inscription_ids,
            rune_summary,
        });
    }

    println!("utxo_count: {}", labeled.len());
    println!(
        "ord_labeling: {}",
        if use_ord { "enabled" } else { "disabled (pass --ord or set PHECHAN_ORD_URL)" }
    );
    if use_ord {
        println!("ord_hits: {ord_hits}");
        println!("ord_misses: {ord_misses}");
        if let Ok(url) = std::env::var("PHECHAN_ORD_URL") {
            println!("ord_url: {url}");
        } else {
            println!("ord_url: http://127.0.0.1:80");
        }
    }
    for line in format_disclosure(&labeled) {
        println!("disclosure: {line}");
    }
    Ok(())
}

fn inspect(args: &[String]) -> Result<(), String> {
    let txid = flag_value(args, "--txid").ok_or("--txid required")?;
    let vout: u32 = flag_value(args, "--vout")
        .ok_or("--vout required")?
        .parse()
        .map_err(|_| "bad --vout")?;
    let rpc = BitcoindRpc::new(RpcConfig::from_env());
    let tx = rpc
        .get_raw_transaction_verbose(&txid)
        .map_err(|e| e.to_string())?;
    let vouts = tx
        .get("vout")
        .and_then(|v| v.as_array())
        .ok_or("missing vout")?;
    let out = vouts
        .iter()
        .find(|v| v.get("n").and_then(|n| n.as_u64()) == Some(vout as u64))
        .ok_or("vout not found")?;
    println!("{}", serde_json::to_string_pretty(out).map_err(|e| e.to_string())?);

    if has_flag(args, "--ord") || std::env::var("PHECHAN_ORD_URL").is_ok() {
        let info = OrdClient::from_env().output_best_effort(&txid, vout);
        println!("ord_found: {}", info.found);
        println!("ord_inscriptions: {}", info.inscription_ids.join(", "));
        if let Some(r) = info.rune_summary {
            println!("ord_runes: {r}");
        }
    }
    Ok(())
}
