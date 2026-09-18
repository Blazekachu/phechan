use std::env;
use std::path::PathBuf;

use phechan_vault::{
    load_vault, save_vault, OutputTemplate, ParentOutpoint, Vault, VaultState,
};

use crate::args::{flag_value, has_flag};

pub fn dispatch(args: &[String]) -> Result<(), String> {
    match args.get(1).map(String::as_str) {
        Some("status") => status(),
        Some("fund") => fund(&args[2..]),
        Some("open-batch") => open_batch(&args[2..]),
        Some("reserve") => reserve(&args[2..]),
        Some("seal") => seal(&args[2..]),
        Some("collect-sig") => collect_sig(),
        Some("broadcast-ready") => broadcast_ready(),
        Some("settle") => settle(&args[2..]),
        Some("abort") => abort(),
        Some("close") => close(),
        Some(other) => Err(format!(
            "unknown vault subcommand '{other}' (status|fund|open-batch|reserve|seal|collect-sig|broadcast-ready|settle|abort|close)"
        )),
        None => Err(
            "usage: phechan vault <status|fund|open-batch|reserve|seal|collect-sig|broadcast-ready|settle|abort|close>"
                .into(),
        ),
    }
}

fn root() -> PathBuf {
    env::var("PHECHAN_VAULT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn with_vault<F>(f: F) -> Result<(), String>
where
    F: FnOnce(&mut Vault) -> Result<(), String>,
{
    let root = root();
    let mut v = load_vault(&root).map_err(|e| e.to_string())?;
    f(&mut v)?;
    save_vault(&root, &v).map_err(|e| e.to_string())?;
    print_status(&v);
    Ok(())
}

fn print_status(v: &Vault) {
    println!("state: {:?}", v.state);
    println!("network: {}", v.network);
    println!("binding: {}", v.binding_summary());
    if let Some(p) = &v.parent {
        println!(
            "parent: {}:{} value={} id={}",
            p.txid, p.vout, p.value_sats, p.parent_inscription_id
        );
    }
    if let Some(b) = &v.batch {
        println!("batch_id: {}", b.id);
        println!("reservations: {}/{}", b.reservations.len(), b.cap);
        println!("collected_sigs: {}", b.collected_sigs);
        println!("template_sealed: {}", b.template.is_some());
        for r in &b.reservations {
            println!("reserved: {} {}", r.minter_id, r.funding_outpoint);
        }
    }
}

fn status() -> Result<(), String> {
    let v = load_vault(&root()).map_err(|e| e.to_string())?;
    print_status(&v);
    Ok(())
}

fn fund(args: &[String]) -> Result<(), String> {
    let txid = flag_value(args, "--txid").ok_or("--txid required")?;
    let vout: u32 = flag_value(args, "--vout")
        .ok_or("--vout required")?
        .parse()
        .map_err(|_| "bad --vout")?;
    let value: u64 = flag_value(args, "--value")
        .ok_or("--value <sats> required")?
        .parse()
        .map_err(|_| "bad --value")?;
    let parent_id = flag_value(args, "--parent-id").ok_or("--parent-id required")?;
    let network = flag_value(args, "--network").unwrap_or_else(|| "regtest".into());
    with_vault(|v| {
        v.network = network;
        v.fund(ParentOutpoint {
            txid,
            vout,
            value_sats: value,
            parent_inscription_id: parent_id,
        })
        .map_err(|e| e.to_string())
    })
}

fn open_batch(args: &[String]) -> Result<(), String> {
    let id = flag_value(args, "--id").ok_or("--id required")?;
    let cap: usize = flag_value(args, "--cap")
        .unwrap_or_else(|| "10".into())
        .parse()
        .map_err(|_| "bad --cap")?;
    with_vault(|v| v.open_batch(id, cap).map_err(|e| e.to_string()))
}

fn reserve(args: &[String]) -> Result<(), String> {
    let minter = flag_value(args, "--minter").ok_or("--minter required")?;
    let outpoint = flag_value(args, "--outpoint").ok_or("--outpoint txid:vout required")?;
    with_vault(|v| v.reserve(minter, outpoint).map_err(|e| e.to_string()))
}

fn seal(args: &[String]) -> Result<(), String> {
    let outputs = flag_value(args, "--outputs").ok_or("--outputs a,b,c required")?;
    let output_values: Vec<u64> = outputs
        .split(',')
        .map(|s| s.trim().parse::<u64>())
        .collect::<Result<_, _>>()
        .map_err(|_| "bad --outputs")?;
    let vault_vout: usize = flag_value(args, "--vault-vout")
        .unwrap_or_else(|| "0".into())
        .parse()
        .map_err(|_| "bad --vault-vout")?;
    let parent_input_index: usize = flag_value(args, "--parent-vin")
        .unwrap_or_else(|| "0".into())
        .parse()
        .map_err(|_| "bad --parent-vin")?;
    with_vault(|v| {
        v.seal(OutputTemplate {
            output_values,
            vault_vout,
            parent_input_index,
            parent_sat_offset: 0,
        })
        .map_err(|e| e.to_string())
    })
}

fn collect_sig() -> Result<(), String> {
    with_vault(|v| v.collect_sig().map_err(|e| e.to_string()))
}

fn broadcast_ready() -> Result<(), String> {
    with_vault(|v| {
        if !has_flag(&[], "--force") {
            // no-op flag check kept for future
        }
        v.mark_broadcast().map_err(|e| e.to_string())
    })
}

fn settle(args: &[String]) -> Result<(), String> {
    let txid = flag_value(args, "--txid").ok_or("--txid required")?;
    let vout: u32 = flag_value(args, "--vout")
        .ok_or("--vout required")?
        .parse()
        .map_err(|_| "bad --vout")?;
    let value: u64 = flag_value(args, "--value")
        .ok_or("--value required")?
        .parse()
        .map_err(|_| "bad --value")?;
    let parent_id = flag_value(args, "--parent-id").ok_or("--parent-id required")?;
    with_vault(|v| {
        v.settle(ParentOutpoint {
            txid,
            vout,
            value_sats: value,
            parent_inscription_id: parent_id,
        })
        .map_err(|e| e.to_string())
    })
}

fn abort() -> Result<(), String> {
    with_vault(|v| v.abort_batch().map_err(|e| e.to_string()))
}

fn close() -> Result<(), String> {
    with_vault(|v| v.close().map_err(|e| e.to_string()))
}

#[allow(dead_code)]
fn _state_name(s: VaultState) -> &'static str {
    match s {
        VaultState::Unfunded => "UNFUNDED",
        VaultState::Ready => "READY",
        VaultState::Reserving => "RESERVING",
        VaultState::Assembled => "ASSEMBLED",
        VaultState::PartialSig => "PARTIAL_SIG",
        VaultState::Broadcast => "BROADCAST",
        VaultState::Settled => "SETTLED",
        VaultState::Quarantine => "QUARANTINE",
        VaultState::Closed => "CLOSED",
    }
}
