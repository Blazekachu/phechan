use bitcoin::consensus::encode::serialize_hex;
use bitcoin::hashes::Hash;
use bitcoin::key::PublicKey;
use bitcoin::psbt::Psbt;
use bitcoin::{Address, Amount, ScriptBuf, Txid, XOnlyPublicKey};
use base64::Engine;
use phechan_bitcoin::{
    build_commit_output, esplora_tip_for_locktime, find_vout_for_address,
    grind_locktime_affixes_final, locktime_is_final, with_lock_time, BitcoindRpc, CommitOutput,
    Network, RpcConfig,
};
use phechan_keystore::{derive_regtest_key, RegtestKey};
use phechan_ordinals::{
    brotli_compress, brotli_recommended_for, build_delegate_tapscript,
    build_inscription_tapscript_opts, build_inscription_tapscript_with_parent, build_text_envelope,
    EnvelopeOptions,
};
use phechan_psbt::{
    build_commit_psbt_multi, build_parent_child_reveal_psbt, build_reveal_psbt, finalize_to_tx,
    sign_reveal_script_path, sign_reveal_script_path_at, txid_grind_template, CommitFundingInput,
    ParentChildRevealParams, RevealPsbtParams,
};
use phechan_runes::{LabeledUtxo, UtxoAssetHint};
use phechan_sat::ParentPlacementPolicy;
use phechan_validation::{
    validate_inscription_reveal, validate_parent_child_layout, ParentChildValidationInput,
};
use std::str::FromStr;

use crate::args::{flag_value, has_flag};
use crate::mainnet_gate::require_mainnet_broadcast_gate;

/// Conservative P2TR dust (policy, not consensus). Postage must be ≥ this.
const P2TR_DUST_SATS: u64 = 330;
const DEFAULT_POSTAGE_SATS: u64 = 546;
const DEFAULT_FEE_RATE_SATS_VB: f64 = 1.0;

fn dust_for_spk(spk: &bitcoin::Script) -> u64 {
    if spk.is_p2tr() {
        330
    } else if spk.is_p2wpkh() {
        294
    } else if spk.is_p2sh() {
        540
    } else {
        546
    }
}

fn funding_input_label(spk: &bitcoin::Script) -> &'static str {
    if spk.is_p2tr() {
        "p2tr"
    } else if spk.is_p2wpkh() {
        "p2wpkh"
    } else if spk.is_p2sh() {
        "p2sh-p2wpkh"
    } else if spk.is_p2pkh() {
        "p2pkh"
    } else {
        "unknown"
    }
}

/// Nested P2SH-P2WPKH redeem = OP_0 <20-byte wpkh>. Matches runes-etch / sort-utxo.
fn nested_p2wpkh_from_pubkey(pubkey_hex: &str) -> Result<(ScriptBuf, ScriptBuf), String> {
    let mut raw = hex::decode(pubkey_hex.trim()).map_err(|e| format!("funding-pubkey-hex: {e}"))?;
    // Xverse may return 32-byte x-only; prepend even-y compressed prefix like runes-etch.
    if raw.len() == 32 {
        let mut compressed = Vec::with_capacity(33);
        compressed.push(0x02);
        compressed.extend_from_slice(&raw);
        raw = compressed;
    }
    if raw.len() != 33 {
        return Err(format!(
            "funding-pubkey-hex: expected 32 or 33 bytes, got {}",
            raw.len()
        ));
    }
    let pk = PublicKey::from_slice(&raw).map_err(|e| format!("funding-pubkey-hex: {e}"))?;
    let wpkh = pk
        .wpubkey_hash()
        .map_err(|e| format!("funding-pubkey-hex wpkh: {e}"))?;
    let redeem = ScriptBuf::new_p2wpkh(&wpkh);
    let p2sh_spk = ScriptBuf::new_p2sh(&redeem.script_hash());
    Ok((redeem, p2sh_spk))
}

/// User-facing funding: postage (inscription output) + fee rate → derive reveal fee & commit.
struct RevealFunding {
    postage_sats: u64,
    fee_sats: u64,
    commit_sats: u64,
}

fn parse_postage(args: &[String]) -> Result<u64, String> {
    let postage = match flag_value(args, "--postage") {
        Some(s) => s.parse::<u64>().map_err(|_| "invalid --postage".to_string())?,
        None => DEFAULT_POSTAGE_SATS,
    };
    if postage < P2TR_DUST_SATS {
        return Err(format!(
            "postage {postage} below P2TR dust floor (~{P2TR_DUST_SATS}); raise padding"
        ));
    }
    Ok(postage)
}

/// Chain tip for locktime vanity — wallet network decides the source.
/// Regtest / explicit PHECHAN_RPC_URL → local bitcoind; otherwise public Esplora
/// (mainnet/signet/testnet never fall back to the regtest :18444 default).
fn tip_for_vanity(network: Network) -> Result<(u32, u32), String> {
    let rpc_configured = std::env::var("PHECHAN_RPC_URL")
        .ok()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let try_local = matches!(network, Network::Regtest) || rpc_configured;
    if try_local {
        match BitcoindRpc::new(RpcConfig::from_env()).get_tip_for_locktime() {
            Ok(tip) => {
                println!("vanity_tip_via: bitcoind");
                return Ok(tip);
            }
            Err(e) if matches!(network, Network::Regtest) => {
                return Err(format!(
                    "vanity needs local regtest tip ({e}); is bitcoind on :18444?"
                ));
            }
            Err(_) => { /* fall through to Esplora on public nets */ }
        }
    }
    match network {
        Network::Regtest => Err(
            "vanity on regtest needs local bitcoind (PHECHAN_RPC_* / :18444)".into(),
        ),
        other => esplora_tip_for_locktime(other)
            .map_err(|e| {
                format!(
                    "vanity needs {other:?} chain tip via Esplora ({e}); wallet network is {other:?}"
                )
            })
            .map(|(h, t)| {
                println!("vanity_tip_via: esplora");
                (h, t)
            }),
    }
}


/// Fee in sats for `vsize` at `fee_rate_sats_vb`.
/// Works for any rate > 0 (e.g. 0.1, 0.69, 1, 42). Uses ceil so we never underfund.
fn fee_from_vsize(vsize: u64, fee_rate_sats_vb: f64) -> u64 {
    if !(fee_rate_sats_vb.is_finite() && fee_rate_sats_vb > 0.0) || vsize == 0 {
        return 1;
    }
    let fee = (vsize as f64 * fee_rate_sats_vb).ceil();
    if !fee.is_finite() || fee < 1.0 {
        1
    } else if fee >= u64::MAX as f64 {
        u64::MAX
    } else {
        fee as u64
    }
}

fn parse_fee_rate(args: &[String]) -> Result<Option<f64>, String> {
    match flag_value(args, "--fee-rate") {
        Some(s) => Ok(Some(parse_positive_fee_rate(&s, "--fee-rate")?)),
        None => Ok(None),
    }
}

fn parse_positive_fee_rate(s: &str, flag: &str) -> Result<f64, String> {
    let r: f64 = s
        .parse()
        .map_err(|_| format!("invalid {flag}"))?;
    if !(r.is_finite() && r > 0.0) {
        return Err(format!("{flag} must be > 0 (fractional sat/vB allowed, e.g. 0.69)"));
    }
    Ok(r)
}

/// Match inscribe.dev (Wizards of Ord) commit-size model — fractional vbytes.
/// See https://inscribe.dev/js application: BASE_TX_SIZE + TAPROOT_* + payment sizes.
const INSCRIBE_BASE_TX_VBYTES: f64 = 10.5;
const INSCRIBE_TAPROOT_INPUT_VBYTES: f64 = 57.5;
const INSCRIBE_TAPROOT_OUTPUT_VBYTES: f64 = 43.0;

/// Payment / change input weight by address type (native + nested segwit, all networks).
fn payment_input_vbytes_f(address: &str) -> f64 {
    let a = address.trim().to_ascii_lowercase();
    if a.starts_with("bc1p") || a.starts_with("tb1p") || a.starts_with("bcrt1p") {
        57.5 // P2TR
    } else if a.starts_with("bc1q") || a.starts_with("tb1q") || a.starts_with("bcrt1q") {
        67.75 // native P2WPKH
    } else if a.starts_with('3') || a.starts_with('2') {
        91.0 // nested P2SH-P2WPKH (Xverse legacy payment)
    } else if a.starts_with('1') || a.starts_with('m') || a.starts_with('n') {
        148.0 // legacy P2PKH
    } else {
        67.75 // default native segwit
    }
}

fn payment_output_vbytes_f(address: &str) -> f64 {
    let a = address.trim().to_ascii_lowercase();
    if a.starts_with("bc1p") || a.starts_with("tb1p") || a.starts_with("bcrt1p") {
        43.0
    } else if a.starts_with("bc1q") || a.starts_with("tb1q") || a.starts_with("bcrt1q") {
        31.0
    } else if a.starts_with('3') || a.starts_with('2') {
        32.0
    } else if a.starts_with('1') || a.starts_with('m') || a.starts_with('n') {
        34.0
    } else {
        31.0
    }
}

/// Commit funding vsize à la inscribe.dev `selectCommitUTXOs` initial sizing.
/// `carrier`: reinscribe / rare-sat / same-sat parent vin0 (taproot).
/// Always includes one payment input + change (conservative; matches typical Xverse flow).
fn estimate_commit_funding_vbytes_inscribe(carrier: bool, payment_address: Option<&str>) -> u64 {
    let pay = payment_address.unwrap_or("bc1q");
    let mut tx_size = INSCRIBE_BASE_TX_VBYTES
        + INSCRIBE_TAPROOT_OUTPUT_VBYTES // commit output
        + payment_output_vbytes_f(pay); // change
    if carrier {
        tx_size += INSCRIBE_TAPROOT_INPUT_VBYTES;
    }
    tx_size += payment_input_vbytes_f(pay);
    tx_size.ceil() as u64
}

/// Attach a dummy script-path witness and return measured vsize (wallet-custody path).
fn vsize_with_dummy_script_path_witness(
    mut tx: bitcoin::Transaction,
    input_index: usize,
    leaf_script: &ScriptBuf,
    control: &bitcoin::taproot::ControlBlock,
) -> u64 {
    use bitcoin::Witness;
    let mut w = Witness::new();
    w.push([0u8; 64]); // schnorr (SIGHASH_DEFAULT)
    w.push(leaf_script.as_bytes());
    w.push(control.serialize());
    if let Some(inp) = tx.input.get_mut(input_index) {
        inp.witness = w;
    }
    tx.vsize() as u64
}

/// Estimate reveal miner fee by measuring a fully-witnessed reveal template.
/// Parent-child (FI/FO): measure 2-in template (not a flat +110 guess).
fn estimate_reveal_fee_sats(
    commit: &CommitOutput,
    keystore: Option<&RegtestKey>,
    postage_sats: u64,
    fee_rate_sats_vb: f64,
    op_return: Option<&[u8]>,
    parent_child: bool,
) -> Result<(u64 /*fee*/, u64 /*vsize*/), String> {
    let provisional_fee = fee_from_vsize(200, fee_rate_sats_vb).max(200);
    let provisional_commit = postage_sats.saturating_add(provisional_fee);
    let mock_commit_txid = Txid::from_byte_array([7u8; 32]);

    let vsize = if parent_child {
        use bitcoin::taproot::LeafVersion;
        let control = commit
            .spend_info
            .control_block(&(commit.leaf_script.clone(), LeafVersion::TapScript))
            .ok_or_else(|| "missing control block for fee estimate".to_string())?;
        let mock_parent_txid = Txid::from_byte_array([8u8; 32]);
        let dest_spk = commit.address.script_pubkey();
        let psbt = build_parent_child_reveal_psbt(ParentChildRevealParams {
            parent_txid: mock_parent_txid,
            parent_vout: 0,
            parent_value: Amount::from_sat(postage_sats.max(P2TR_DUST_SATS)),
            parent_script_pubkey: dest_spk.clone(),
            parent_tap_internal_key: Some(commit.spend_info.internal_key()),
            commit_txid: mock_commit_txid,
            commit_vout: 0,
            commit_value: Amount::from_sat(provisional_commit),
            commit_script_pubkey: commit.script_pubkey.clone(),
            vault_script_pubkey: dest_spk.clone(),
            vault_value: Amount::from_sat(postage_sats.max(P2TR_DUST_SATS)),
            child_script_pubkey: dest_spk,
            child_value: Amount::from_sat(postage_sats),
            leaf_script: commit.leaf_script.clone(),
            spend_info: commit.spend_info.clone(),
        })
        .map_err(|e| e.to_string())?;
        let mut tx = psbt.unsigned_tx;
        // Parent key-path witness
        {
            use bitcoin::Witness;
            let mut w = Witness::new();
            w.push([0u8; 64]);
            tx.input[0].witness = w;
        }
        // OP_RETURN on reveal (inscribe.dev adds this when enabled) — size matters at low fee rates.
        if let Some(data) = op_return {
            if !data.is_empty() && data.len() <= 80 {
                if let Ok(push) = <&bitcoin::script::PushBytes>::try_from(data) {
                    tx.output.push(bitcoin::TxOut {
                        value: Amount::ZERO,
                        script_pubkey: ScriptBuf::new_op_return(push),
                    });
                }
            }
        }
        vsize_with_dummy_script_path_witness(tx, 1, &commit.leaf_script, &control)
    } else {
        let psbt = build_reveal_psbt(RevealPsbtParams {
            commit_txid: mock_commit_txid,
            commit_vout: 0,
            commit_value: Amount::from_sat(provisional_commit),
            commit_script_pubkey: commit.script_pubkey.clone(),
            destination_script_pubkey: commit.address.script_pubkey(),
            destination_value: Amount::from_sat(postage_sats),
            leaf_script: commit.leaf_script.clone(),
            spend_info: commit.spend_info.clone(),
            op_return: op_return.map(|d| d.to_vec()),
        })
        .map_err(|e| e.to_string())?;

        if let Some(key) = keystore {
            let signed = sign_reveal_script_path(psbt, &key.keypair, &commit.leaf_script)
                .map_err(|e| e.to_string())?;
            let tx = finalize_to_tx(&signed).map_err(|e| e.to_string())?;
            tx.vsize() as u64
        } else {
            use bitcoin::taproot::LeafVersion;
            let control = commit
                .spend_info
                .control_block(&(commit.leaf_script.clone(), LeafVersion::TapScript))
                .ok_or_else(|| "missing control block for fee estimate".to_string())?;
            vsize_with_dummy_script_path_witness(
                psbt.unsigned_tx,
                0,
                &commit.leaf_script,
                &control,
            )
        }
    };

    let fee = fee_from_vsize(vsize, fee_rate_sats_vb);
    Ok((fee, vsize))
}

/// Parse optional `--op-return` UTF-8 message (≤80 bytes) for reveal nulldata output.
fn parse_op_return(args: &[String]) -> Result<Option<Vec<u8>>, String> {
    let Some(s) = flag_value(args, "--op-return") else {
        return Ok(None);
    };
    let bytes = s.into_bytes();
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() > 80 {
        return Err(format!(
            "OP_RETURN is {} bytes; standard relay limit is 80",
            bytes.len()
        ));
    }
    Ok(Some(bytes))
}

fn resolve_reveal_funding(
    args: &[String],
    commit: &CommitOutput,
    keystore: Option<&RegtestKey>,
    _network: Network,
) -> Result<RevealFunding, String> {
    let postage_sats = parse_postage(args)?;
    let fee_rate_opt = parse_fee_rate(args)?;

    // Prefer postage + fee-rate (UI path). Legacy --fee-sats/--commit-sats still work for CLI.
    if fee_rate_opt.is_some()
        || flag_value(args, "--postage").is_some()
        || (flag_value(args, "--fee-sats").is_none() && flag_value(args, "--commit-sats").is_none())
    {
        let fee_rate = fee_rate_opt.unwrap_or(DEFAULT_FEE_RATE_SATS_VB);
        let op_return = parse_op_return(args)?;
        let same_sat = has_flag(args, "--same-sat-parent");
        let parent_child = !same_sat
            && (flag_value(args, "--parent-outpoint").is_some()
                || flag_value(args, "--parent").is_some());
        let (fee_sats, vsize) = if let Some(flat) = flag_value(args, "--fee-sats") {
            let f = flat.parse::<u64>().map_err(|_| "invalid --fee-sats".to_string())?;
            (f, 0u64)
        } else {
            estimate_reveal_fee_sats(
                commit,
                keystore,
                postage_sats,
                fee_rate,
                op_return.as_deref(),
                parent_child,
            )?
        };
        if parent_child {
            println!("parent_child_reveal: true");
        }
        if same_sat {
            println!("same_sat_parent: true");
            println!(
                "note: parent sat must be funded into commit; reveal spends commit (parent UTXO) + tag 3"
            );
        }
        let commit_sats = if let Some(c) = flag_value(args, "--commit-sats") {
            c.parse::<u64>().map_err(|_| "invalid --commit-sats".to_string())?
        } else {
            postage_sats.saturating_add(fee_sats)
        };
        if commit_sats < postage_sats.saturating_add(1) {
            return Err("commit funding must exceed postage".into());
        }
        // Commit tx miner fee — align with inscribe.dev selectCommitUTXOs sizing.
        // Postage is NOT a network fee. Carrier (reinscribe/sat) adds a taproot input.
        let commit_fee_rate = match flag_value(args, "--commit-fee-rate") {
            Some(s) => parse_positive_fee_rate(&s, "--commit-fee-rate")?,
            None => fee_rate,
        };
        let carrier = has_flag(args, "--commit-estimate-carrier")
            || has_flag(args, "--same-sat-parent")
            || flag_value(args, "--satpoint-hint").is_some()
            || flag_value(args, "--reinscribe-id").is_some();
        // Prefer real payment/change address — never ordinals destination (wrong vin size).
        let payment_addr = flag_value(args, "--payment-address")
            .or_else(|| flag_value(args, "--change-address"));
        let commit_vbytes = estimate_commit_funding_vbytes_inscribe(carrier, payment_addr.as_deref());
        let commit_fee_estimate_sats = fee_from_vsize(commit_vbytes, commit_fee_rate);
        let network_fee_sats = fee_sats.saturating_add(commit_fee_estimate_sats);

        if vsize > 0 {
            println!("reveal_vsize: {vsize}");
        }
        println!("fee_rate_sats_vb: {fee_rate}");
        println!("commit_fee_rate_sats_vb: {commit_fee_rate}");
        println!("postage_sats: {postage_sats}");
        println!("note: postage returns to you with the inscription — not a miner fee");
        println!("reveal_fee_sats: {fee_sats}");
        println!("commit_estimate_carrier: {carrier}");
        println!("commit_fee_estimate_sats: {commit_fee_estimate_sats}");
        println!("commit_fee_estimate_vbytes: {commit_vbytes}");
        println!("network_fee_sats: {network_fee_sats}");
        println!("commit_sats: {commit_sats}");
        println!("note: commit_sats = postage + reveal_fee (amount to send to commit address)");
        println!("note: network_fee_sats = reveal_fee + commit_fee (miner fees; matches inscribe.dev Network Fees)");
        return Ok(RevealFunding {
            postage_sats,
            fee_sats,
            commit_sats,
        });
    }

    // Legacy absolute fee + commit → implied postage
    let commit_sats: u64 = flag_value(args, "--commit-sats")
        .unwrap_or_else(|| "10000".into())
        .parse()
        .map_err(|_| "invalid --commit-sats")?;
    let fee_sats: u64 = flag_value(args, "--fee-sats")
        .unwrap_or_else(|| "500".into())
        .parse()
        .map_err(|_| "invalid --fee-sats")?;
    if commit_sats <= fee_sats + P2TR_DUST_SATS {
        return Err("commit value too small for fee + dust".into());
    }
    let postage_sats = commit_sats.saturating_sub(fee_sats);
    println!("postage_sats: {postage_sats} (derived from --commit-sats − --fee-sats)");
    println!("reveal_fee_sats: {fee_sats}");
    println!("commit_sats: {commit_sats}");
    Ok(RevealFunding {
        postage_sats,
        fee_sats,
        commit_sats,
    })
}

pub fn dispatch(args: &[String]) -> Result<(), String> {
    match args.get(1).map(String::as_str) {
        Some("create") => create(&args[2..]),
        Some("child") => child(&args[2..]),
        Some("delegate") => delegate(&args[2..]),
        Some("reinscribe") => reinscribe(&args[2..]),
        Some("inspect") => inspect(&args[2..]),
        Some("fund-commit") => fund_commit(&args[2..]),
        Some("recover-reveal") => recover_reveal(&args[2..]),
        Some(other) => Err(format!(
            "unknown inscription subcommand '{other}' (create|child|delegate|reinscribe|inspect|fund-commit|recover-reveal)"
        )),
        None => Err(
            "usage: phechan inscription <create|child|delegate|reinscribe|inspect|fund-commit|recover-reveal>"
                .into(),
        ),
    }
}

fn create(args: &[String]) -> Result<(), String> {
    let body_owned: Vec<u8> = if let Some(path) = flag_value(args, "--body-file") {
        std::fs::read(path.trim()).map_err(|e| format!("--body-file read {path}: {e}"))?
    } else if let Some(h) = flag_value(args, "--body-hex") {
        hex::decode(h.trim()).map_err(|e| e.to_string())?
    } else {
        flag_value(args, "--body")
            .unwrap_or_else(|| "Hello, world!".to_string())
            .into_bytes()
    };
    let parent = flag_value(args, "--parent");
    run_single_reveal(
        args,
        &body_owned,
        parent.as_deref(),
        None,
        "inscription-create",
    )
}

fn delegate(args: &[String]) -> Result<(), String> {
    let delegate_id =
        flag_value(args, "--delegate").ok_or("--delegate <inscription_id> required")?;
    let body = flag_value(args, "--body");
    let body_bytes = body.as_deref().map(|s| s.as_bytes());
    println!("note: delegate resolves content via tag 11; may 404 until target exists");
    run_single_reveal(
        args,
        body_bytes.unwrap_or(b""),
        None,
        Some(delegate_id.as_str()),
        "inscription-delegate",
    )
}

fn reinscribe(args: &[String]) -> Result<(), String> {
    let satpoint = flag_value(args, "--satpoint").ok_or(
        "--satpoint <txid:vout> required (reinscription targets an already-inscribed sat UTXO)",
    )?;
    parse_outpoint(&satpoint)?;
    let body_owned: Vec<u8> = if let Some(path) = flag_value(args, "--body-file") {
        std::fs::read(path.trim()).map_err(|e| format!("--body-file read {path}: {e}"))?
    } else if let Some(h) = flag_value(args, "--body-hex") {
        hex::decode(h.trim()).map_err(|e| e.to_string())?
    } else {
        flag_value(args, "--body")
            .unwrap_or_else(|| "reinscription".to_string())
            .into_bytes()
    };
    println!("note: reinscription APPENDS; it does not overwrite prior inscriptions on the sat");
    println!("satpoint: {satpoint}");
    println!(
        "disclosure: fund-commit must spend this UTXO as --carrier-* (vin0) so the sat enters commit"
    );
    run_single_reveal(
        args,
        &body_owned,
        None,
        None,
        "inscription-reinscribe",
    )
}

/// Rough input weight in vbytes (signed) from scriptPubKey type.
/// Aligned with inscribe.dev calculateInputSize (ceiled to whole vbytes).
fn estimate_input_vbytes(spk: &bitcoin::Script) -> u64 {
    if spk.is_p2tr() {
        58 // 57.5
    } else if spk.is_p2wpkh() {
        68 // 67.75
    } else if spk.is_p2wsh() {
        110
    } else if spk.is_p2sh() {
        91 // nested P2WPKH
    } else {
        148 // legacy P2PKH
    }
}

fn estimate_output_vbytes(spk: &bitcoin::Script) -> u64 {
    if spk.is_p2tr() {
        43
    } else if spk.is_p2wpkh() {
        31
    } else if spk.is_p2sh() {
        32
    } else {
        34
    }
}

fn estimate_commit_funding_vbytes(
    funding_spk: &bitcoin::Script,
    commit_spk: &bitcoin::Script,
    change_spk: Option<&bitcoin::Script>,
) -> u64 {
    estimate_commit_funding_vbytes_multi(&[funding_spk], commit_spk, change_spk)
}

fn estimate_commit_funding_vbytes_multi(
    input_spks: &[&bitcoin::Script],
    commit_spk: &bitcoin::Script,
    change_spk: Option<&bitcoin::Script>,
) -> u64 {
    // Match inscribe.dev: BASE 10.5 → ceil with inputs/outputs.
    let mut v = INSCRIBE_BASE_TX_VBYTES;
    for spk in input_spks {
        v += estimate_input_vbytes(spk) as f64;
    }
    v += estimate_output_vbytes(commit_spk) as f64;
    if let Some(c) = change_spk {
        v += estimate_output_vbytes(c) as f64;
    }
    v.ceil() as u64
}

/// Build unsigned commit-funding PSBT (wallet signs). Optional commit TXID vanity via locktime.
///
/// Same-sat parent: pass `--carrier-*` as vin0 (parent/ordinals UTXO). Optional `--funding-*`
/// is payment top-up as vin1 when the parent alone cannot cover commit + fee.
fn fund_commit(args: &[String]) -> Result<(), String> {
    let network =
        Network::parse(&flag_value(args, "--network").unwrap_or_else(|| "signet".into()))
            .ok_or("invalid --network")?;
    let commit_addr_s =
        flag_value(args, "--commit-address").ok_or("--commit-address required")?;
    let change_addr_s =
        flag_value(args, "--change-address").ok_or("--change-address required")?;
    let commit_sats: u64 = flag_value(args, "--commit-sats")
        .ok_or("--commit-sats required")?
        .parse()
        .map_err(|_| "invalid --commit-sats")?;
    let fee_rate = parse_fee_rate(args)?.unwrap_or(DEFAULT_FEE_RATE_SATS_VB);

    let commit_addr = Address::from_str(&commit_addr_s)
        .map_err(|e| e.to_string())?
        .require_network(network.to_bitcoin())
        .map_err(|e| e.to_string())?;
    let change_addr = Address::from_str(&change_addr_s)
        .map_err(|e| e.to_string())?
        .require_network(network.to_bitcoin())
        .map_err(|e| e.to_string())?;
    let commit_spk = commit_addr.script_pubkey();
    let change_spk = change_addr.script_pubkey();
    let change_dust = dust_for_spk(&change_spk);

    let carrier = parse_optional_carrier_input(args, network)?;
    let payment = parse_optional_payment_funding(args, network)?;

    let ((psbt_inputs, use_change, change_sats, fee_sats, vsize), input_type, note) =
        match (carrier, payment) {
            (Some(c), Some(p)) => {
                let spk_c = c.script_pubkey.clone();
                let spk_p = p.script_pubkey.clone();
                let spks: [&bitcoin::Script; 2] = [spk_c.as_ref(), spk_p.as_ref()];
                let mut vsize =
                    estimate_commit_funding_vbytes_multi(&spks, &commit_spk, Some(&change_spk));
                let mut fee_sats = fee_from_vsize(vsize, fee_rate);
                fee_sats = fee_sats.max(1);
                let total_in = c.value.to_sat() + p.value.to_sat();
                if total_in < commit_sats.saturating_add(fee_sats) {
                    return Err(format!(
                        "carrier+payment {} sats too small for commit {commit_sats} + fee ~{fee_sats}",
                        total_in
                    ));
                }
                let mut change_sats = total_in - commit_sats - fee_sats;
                let use_change = change_sats >= change_dust;
                if !use_change {
                    vsize = estimate_commit_funding_vbytes_multi(&spks, &commit_spk, None);
                    fee_sats = total_in - commit_sats;
                    change_sats = 0;
                }
                println!("same_sat_carrier: true");
                (
                    (vec![c, p], use_change, change_sats, fee_sats, vsize),
                    "p2tr+payment",
                    "sign vin0 (ordinals/parent) and vin1 (payment); parent sat → commit vout0",
                )
            }
            (Some(c), None) => {
                let spk_c = c.script_pubkey.clone();
                let spks: [&bitcoin::Script; 1] = [spk_c.as_ref()];
                let mut vsize =
                    estimate_commit_funding_vbytes_multi(&spks, &commit_spk, Some(&change_spk));
                let mut fee_sats = fee_from_vsize(vsize, fee_rate);
                fee_sats = fee_sats.max(1);
                let funding_value = c.value.to_sat();
                if funding_value < commit_sats.saturating_add(fee_sats) {
                    return Err(format!(
                        "parent UTXO {funding_value} sats too small for commit {commit_sats} + fee ~{fee_sats} — add a payment UTXO as funding top-up"
                    ));
                }
                let mut change_sats = funding_value - commit_sats - fee_sats;
                let use_change = change_sats >= change_dust;
                if !use_change {
                    vsize = estimate_commit_funding_vbytes_multi(&spks, &commit_spk, None);
                    fee_sats = funding_value - commit_sats;
                    change_sats = 0;
                }
                println!("same_sat_carrier: true");
                (
                    (vec![c], use_change, change_sats, fee_sats, vsize),
                    "p2tr-carrier",
                    "sign vin0 (ordinals/parent); parent sat → commit vout0",
                )
            }
            (None, _) => {
                let funding_txid_s =
                    flag_value(args, "--funding-txid").ok_or("--funding-txid required")?;
                let funding_vout: u32 = flag_value(args, "--funding-vout")
                    .ok_or("--funding-vout required")?
                    .parse()
                    .map_err(|_| "invalid --funding-vout")?;
                let funding_value: u64 = flag_value(args, "--funding-value")
                    .ok_or("--funding-value required")?
                    .parse()
                    .map_err(|_| "invalid --funding-value")?;
                let funding_spk = if let Some(hex_s) = flag_value(args, "--funding-script-hex") {
                    let raw = hex::decode(hex_s.trim())
                        .map_err(|e| format!("funding-script-hex: {e}"))?;
                    ScriptBuf::from_bytes(raw)
                } else if let Some(fa) = flag_value(args, "--funding-address") {
                    Address::from_str(&fa)
                        .map_err(|e| e.to_string())?
                        .require_network(network.to_bitcoin())
                        .map_err(|e| e.to_string())?
                        .script_pubkey()
                } else {
                    return Err("--funding-script-hex or --funding-address required".into());
                };
                if funding_spk.is_p2pkh() {
                    return Err(
                        "Legacy P2PKH funding UTXOs are not supported. Use native (bc1q/tb1q) or nested (3…/2…) segwit."
                            .into(),
                    );
                }
                let funding_redeem = if funding_spk.is_p2sh() {
                    let pk_hex = flag_value(args, "--funding-pubkey-hex").ok_or(
                        "P2SH (nested segwit) funding requires --funding-pubkey-hex from wallet paymentPublicKey",
                    )?;
                    let (redeem, expected_p2sh) = nested_p2wpkh_from_pubkey(&pk_hex)?;
                    if expected_p2sh != funding_spk {
                        return Err(
                            "funding-pubkey-hex does not match funding scriptPubKey (not this payment address)"
                                .into(),
                        );
                    }
                    Some(redeem)
                } else {
                    None
                };
                let input_type = funding_input_label(&funding_spk);
                let mut vsize =
                    estimate_commit_funding_vbytes(&funding_spk, &commit_spk, Some(&change_spk));
                let mut fee_sats = fee_from_vsize(vsize, fee_rate);
                fee_sats = fee_sats.max(1);
                if funding_value < commit_sats.saturating_add(fee_sats) {
                    return Err(format!(
                        "funding UTXO {funding_value} sats too small for commit {commit_sats} + fee ~{fee_sats} (vsize {vsize} @ {fee_rate} sat/vB, input {input_type})"
                    ));
                }
                let mut change_sats = funding_value - commit_sats - fee_sats;
                let use_change = change_sats >= change_dust;
                if !use_change {
                    vsize = estimate_commit_funding_vbytes(&funding_spk, &commit_spk, None);
                    fee_sats = funding_value - commit_sats;
                    change_sats = 0;
                }
                let funding_txid = Txid::from_str(&funding_txid_s).map_err(|e| e.to_string())?;
                let tap = if funding_spk.is_p2tr() {
                    flag_value(args, "--ordinals-pubkey-hex")
                        .map(|pk| parse_xonly_pubkey(&pk))
                        .transpose()?
                } else {
                    None
                };
                (
                    (
                        vec![CommitFundingInput {
                            outpoint: bitcoin::OutPoint {
                                txid: funding_txid,
                                vout: funding_vout,
                            },
                            value: Amount::from_sat(funding_value),
                            script_pubkey: funding_spk,
                            redeem_script: funding_redeem,
                            tap_internal_key: tap,
                        }],
                        use_change,
                        change_sats,
                        fee_sats,
                        vsize,
                    ),
                    input_type,
                    "sign this PSBT in wallet (payment input), then broadcast; reveal uses --commit-txid",
                )
            }
        };

    fund_commit_finish(
        network,
        args,
        &commit_addr_s,
        commit_sats,
        fee_rate,
        psbt_inputs,
        use_change,
        change_spk,
        change_sats,
        fee_sats,
        vsize,
        input_type,
        note,
        commit_spk,
    )
}

fn parse_optional_carrier_input(
    args: &[String],
    network: Network,
) -> Result<Option<CommitFundingInput>, String> {
    let Some(txid_s) = flag_value(args, "--carrier-txid") else {
        return Ok(None);
    };
    let vout: u32 = flag_value(args, "--carrier-vout")
        .ok_or("--carrier-vout required with --carrier-txid")?
        .parse()
        .map_err(|_| "invalid --carrier-vout")?;
    let value: u64 = flag_value(args, "--carrier-value")
        .ok_or("--carrier-value required with --carrier-txid")?
        .parse()
        .map_err(|_| "invalid --carrier-value")?;
    let spk = if let Some(hex_s) = flag_value(args, "--carrier-script-hex") {
        ScriptBuf::from_bytes(
            hex::decode(hex_s.trim()).map_err(|e| format!("carrier-script-hex: {e}"))?,
        )
    } else if let Some(addr_s) = flag_value(args, "--carrier-address") {
        Address::from_str(&addr_s)
            .map_err(|e| e.to_string())?
            .require_network(network.to_bitcoin())
            .map_err(|e| e.to_string())?
            .script_pubkey()
    } else {
        return Err("--carrier-script-hex or --carrier-address required with --carrier-txid".into());
    };
    if !spk.is_p2tr() {
        return Err("carrier (parent) UTXO must be P2TR ordinals address".into());
    }
    let tap = flag_value(args, "--ordinals-pubkey-hex")
        .map(|pk| parse_xonly_pubkey(&pk))
        .transpose()?;
    if tap.is_none() {
        println!(
            "warning: no --ordinals-pubkey-hex — Xverse may refuse to sign parent carrier without tapInternalKey"
        );
    }
    let txid = Txid::from_str(&txid_s).map_err(|e| e.to_string())?;
    Ok(Some(CommitFundingInput {
        outpoint: bitcoin::OutPoint { txid, vout },
        value: Amount::from_sat(value),
        script_pubkey: spk,
        redeem_script: None,
        tap_internal_key: tap,
    }))
}

fn parse_optional_payment_funding(
    args: &[String],
    network: Network,
) -> Result<Option<CommitFundingInput>, String> {
    // Only treat as payment top-up when carrier is also present.
    if flag_value(args, "--carrier-txid").is_none() {
        return Ok(None);
    }
    let Some(txid_s) = flag_value(args, "--funding-txid") else {
        return Ok(None);
    };
    let vout: u32 = flag_value(args, "--funding-vout")
        .ok_or("--funding-vout required with --funding-txid")?
        .parse()
        .map_err(|_| "invalid --funding-vout")?;
    let value: u64 = flag_value(args, "--funding-value")
        .ok_or("--funding-value required with --funding-txid")?
        .parse()
        .map_err(|_| "invalid --funding-value")?;
    let spk = if let Some(hex_s) = flag_value(args, "--funding-script-hex") {
        ScriptBuf::from_bytes(
            hex::decode(hex_s.trim()).map_err(|e| format!("funding-script-hex: {e}"))?,
        )
    } else if let Some(fa) = flag_value(args, "--funding-address") {
        Address::from_str(&fa)
            .map_err(|e| e.to_string())?
            .require_network(network.to_bitcoin())
            .map_err(|e| e.to_string())?
            .script_pubkey()
    } else {
        return Err("--funding-script-hex or --funding-address required".into());
    };
    if spk.is_p2pkh() {
        return Err("Legacy P2PKH payment top-up not supported".into());
    }
    let redeem = if spk.is_p2sh() {
        let pk_hex = flag_value(args, "--funding-pubkey-hex").ok_or(
            "P2SH payment top-up requires --funding-pubkey-hex",
        )?;
        let (redeem, expected) = nested_p2wpkh_from_pubkey(&pk_hex)?;
        if expected != spk {
            return Err("funding-pubkey-hex does not match payment scriptPubKey".into());
        }
        Some(redeem)
    } else {
        None
    };
    let txid = Txid::from_str(&txid_s).map_err(|e| e.to_string())?;
    Ok(Some(CommitFundingInput {
        outpoint: bitcoin::OutPoint { txid, vout },
        value: Amount::from_sat(value),
        script_pubkey: spk,
        redeem_script: redeem,
        tap_internal_key: None,
    }))
}

fn fund_commit_finish(
    network: Network,
    args: &[String],
    commit_addr_s: &str,
    commit_sats: u64,
    fee_rate: f64,
    inputs: Vec<CommitFundingInput>,
    use_change: bool,
    change_spk: ScriptBuf,
    change_sats: u64,
    fee_sats: u64,
    vsize: u64,
    input_type: &str,
    note: &str,
    commit_spk: ScriptBuf,
) -> Result<(), String> {
    let mut psbt = build_commit_psbt_multi(
        inputs,
        commit_spk,
        Amount::from_sat(commit_sats),
        use_change.then_some(change_spk),
        Amount::from_sat(change_sats),
    )
    .map_err(|e| e.to_string())?;

    let vanity_prefix = flag_value(args, "--vanity-prefix").unwrap_or_default();
    let vanity_suffix = flag_value(args, "--vanity-suffix").unwrap_or_default();
    if !vanity_prefix.is_empty() || !vanity_suffix.is_empty() {
        let template = txid_grind_template(&psbt).map_err(|e| e.to_string())?;
        let max_tries: u32 = flag_value(args, "--vanity-max-tries")
            .unwrap_or_else(|| "5000000".into())
            .parse()
            .map_err(|_| "invalid --vanity-max-tries")?;
        let (tip_height, mediantime) = tip_for_vanity(network)?;
        println!(
            "commit_vanity_grind: prefix={vanity_prefix:?} suffix={vanity_suffix:?} max_tries={max_tries} tip={tip_height} mediantime={mediantime}"
        );
        let (lt, ground_txid) = grind_locktime_affixes_final(
            &template,
            &vanity_prefix,
            &vanity_suffix,
            tip_height,
            mediantime,
            max_tries,
        )
        .ok_or_else(|| {
            format!(
                "commit vanity grind failed within final locktime window for {vanity_prefix}…{vanity_suffix}"
            )
        })?;
        if !locktime_is_final(lt, tip_height, mediantime) {
            return Err(format!(
                "internal: ground locktime {lt} not final at tip={tip_height} mediantime={mediantime}"
            ));
        }
        psbt.unsigned_tx = with_lock_time(psbt.unsigned_tx.clone(), lt);
        println!("commit_vanity_lock_time: {lt}");
        println!("commit_vanity_txid: {ground_txid}");
    } else {
        let preview = txid_grind_template(&psbt)
            .map(|t| t.compute_txid().to_string())
            .unwrap_or_else(|_| psbt.unsigned_tx.compute_txid().to_string());
        println!("commit_txid_preview: {preview}");
    }

    let b64 = base64::engine::general_purpose::STANDARD.encode(psbt.serialize());
    println!("network: {}", network.as_str());
    println!("funding_input_type: {input_type}");
    println!("fee_rate_sats_vb: {fee_rate}");
    println!("funding_vsize_estimate: {vsize}");
    println!("commit_funding_fee_sats: {fee_sats}");
    println!("commit_sats: {commit_sats}");
    println!("change_sats: {change_sats}");
    println!("commit_address: {commit_addr_s}");
    println!("psbt_base64: {b64}");
    println!("note: {note}");
    Ok(())
}

/// Shared create/delegate/reinscribe path (single-input reveal).
fn run_single_reveal(
    args: &[String],
    body: &[u8],
    parent_id: Option<&str>,
    delegate_id: Option<&str>,
    key_label: &str,
) -> Result<(), String> {
    let network =
        Network::parse(&flag_value(args, "--network").unwrap_or_else(|| "regtest".into()))
            .ok_or("invalid --network")?;
    let dry_run = has_flag(args, "--dry-run");
    let broadcast = has_flag(args, "--broadcast");
    let unsigned_psbt = has_flag(args, "--unsigned-psbt");
    if !dry_run && !broadcast && !unsigned_psbt {
        return Err("require --dry-run, --broadcast, or --unsigned-psbt".into());
    }
    // Building unsigned PSBTs / dry-run on mainnet is fine; live broadcast is dual-gated.
    if broadcast {
        require_mainnet_broadcast_gate(args, network)?;
        if network != Network::Regtest
            && network != Network::Signet
            && network != Network::Testnet
            && network != Network::Mainnet
        {
            return Err("live --broadcast supports regtest|signet|testnet|mainnet".into());
        }
    }
    if unsigned_psbt && broadcast {
        return Err("use --unsigned-psbt without --broadcast; then psbt finalize-import --broadcast".into());
    }

    // Self-custody: prefer wallet ordinals x-only pubkey for tapscript CHECKSIG.
    // Phechan never holds that private key — only the connected wallet can reveal / recover.
    // Keystore remains for regtest/signet/testnet automation when no pubkey is passed.
    let (xonly, keystore) = if let Some(pk_hex) = flag_value(args, "--ordinals-pubkey-hex") {
        let pk = parse_xonly_pubkey(&pk_hex)?;
        println!("commit_custody: wallet");
        println!(
            "note: inscription tapscript uses your wallet pubkey — Phechan cannot spend the commit"
        );
        (pk.serialize(), None)
    } else if network == Network::Mainnet {
        return Err(
            "mainnet requires wallet self-custody: pass --ordinals-pubkey-hex (connect Ordinals wallet in UI)"
                .into(),
        );
    } else {
        let key = derive_regtest_key(network, key_label).map_err(|e| e.to_string())?;
        println!("commit_custody: keystore");
        let x = key.xonly.serialize();
        (x, Some(key))
    };

    let content_type = flag_value(args, "--content-type")
        .unwrap_or_else(|| "text/plain;charset=utf-8".into());
    let metaprotocol = flag_value(args, "--metaprotocol");
    let metadata = flag_value(args, "--metadata");
    let title = flag_value(args, "--title");
    let parent_flag = flag_value(args, "--parent");
    let parent_eff = parent_id.or(parent_flag.as_deref());
    let compress_br = has_flag(args, "--compress-br")
        || flag_value(args, "--content-encoding").as_deref() == Some("br");

    if let Some(opr) = parse_op_return(args)? {
        let preview = String::from_utf8_lossy(&opr);
        println!("op_return: {preview} ({} bytes → reveal vout1)", opr.len());
    }
    if let Some(vp) = flag_value(args, "--vanity-prefix") {
        println!("vanity_prefix: {vp}");
    }
    if let Some(vs) = flag_value(args, "--vanity-suffix") {
        println!("vanity_suffix: {vs}");
    }
    if flag_value(args, "--vanity-prefix").is_some() || flag_value(args, "--vanity-suffix").is_some()
    {
        println!("note: vanity grind runs pre-sign on reveal locktime (hex 0-9a-f)");
    }
    if let Some(sn) = flag_value(args, "--sat-number") {
        println!("sat_number_target: {sn}");
        println!("disclosure: sat-number targeting needs indexer sat index; treat as intent for now");
    }

    let compressed_body = if compress_br {
        if !brotli_recommended_for(&content_type) {
            println!(
                "warning: Brotli on content-type {content_type} may not help; clients still need tag 9=br"
            );
        }
        let compressed = brotli_compress(body).map_err(|e| e.to_string())?;
        println!(
            "content_encoding: br ({} → {} bytes)",
            body.len(),
            compressed.len()
        );
        Some(compressed)
    } else {
        None
    };
    let body_ref: &[u8] = compressed_body.as_deref().unwrap_or(body);
    let encoding_br = compress_br.then(|| b"br".to_vec());

    let meta_bytes = metadata.as_ref().map(|s| s.as_bytes());
    let opts = EnvelopeOptions {
        parent_id: parent_eff,
        delegate_id,
        metaprotocol: metaprotocol.as_deref(),
        metadata: meta_bytes,
        title: title.as_deref(),
        content_encoding: encoding_br.as_deref(),
        omit_content: delegate_id.is_some() && body_ref.is_empty() && body.is_empty(),
    };

    let leaf = if let Some(did) = delegate_id {
        if opts.metaprotocol.is_some()
            || opts.parent_id.is_some()
            || opts.metadata.is_some()
            || opts.title.is_some()
        {
            let mut o = opts.clone();
            o.delegate_id.replace(did);
            o.omit_content = body_ref.is_empty() && body.is_empty();
            build_inscription_tapscript_opts(&xonly, content_type.as_bytes(), body_ref, o)
                .map_err(|e| e.to_string())?
        } else {
            build_delegate_tapscript(
                &xonly,
                did,
                if body_ref.is_empty() {
                    None
                } else {
                    Some(body_ref)
                },
            )
            .map_err(|e| e.to_string())?
        }
    } else {
        build_inscription_tapscript_opts(&xonly, content_type.as_bytes(), body_ref, opts)
            .map_err(|e| e.to_string())?
    };
    let commit = build_commit_output(network, &xonly, leaf).map_err(|e| e.to_string())?;

    let funding = resolve_reveal_funding(args, &commit, keystore.as_ref(), network)?;
    let commit_value_sats = funding.commit_sats;
    let postage_sats = funding.postage_sats;
    let fee_sats = funding.fee_sats;

    // Wallet custody cannot keystore-sign. Treat --broadcast as unsigned PSBT for the wallet.
    let mut unsigned_psbt = unsigned_psbt;
    let mut broadcast = broadcast;
    if keystore.is_none() && broadcast && !unsigned_psbt {
        println!("note: wallet custody — returning unsigned reveal PSBT (wallet must sign)");
        unsigned_psbt = true;
        broadcast = false;
    }

    if dry_run && !broadcast {
        let commit_value = Amount::from_sat(commit_value_sats);
        let dest_value = Amount::from_sat(postage_sats);
        let mock_commit_txid = Txid::from_byte_array([7u8; 32]);
        let psbt = build_reveal_psbt(RevealPsbtParams {
            commit_txid: mock_commit_txid,
            commit_vout: 0,
            commit_value,
            commit_script_pubkey: commit.script_pubkey.clone(),
            destination_script_pubkey: commit.address.script_pubkey(),
            destination_value: dest_value,
            leaf_script: commit.leaf_script.clone(),
            spend_info: commit.spend_info.clone(),
            op_return: parse_op_return(args)?,
        })
        .map_err(|e| e.to_string())?;

        if unsigned_psbt {
            let b64 = base64::engine::general_purpose::STANDARD.encode(psbt.serialize());
            println!("network: {}", network.as_str());
            println!("commit_address: {}", commit.address);
            println!("psbt_base64: {b64}");
            println!("note: mock commit outpoint — for wallet UX dry export only");
            println!("next: fund commit, rebuild with live outpoint, or use live --unsigned-psbt");
            return Ok(());
        }

        if let Some(ref key) = keystore {
            let signed = sign_reveal_script_path(psbt, &key.keypair, &commit.leaf_script)
                .map_err(|e| e.to_string())?;
            let tx = finalize_to_tx(&signed).map_err(|e| e.to_string())?;
            let report = if body_ref.is_empty() {
                phechan_validation::ValidationReport {
                    consensus_ok: Some(true),
                    relay_ok: Some(true),
                    ordinals_ok: Some(true),
                    runes_ok: Some(true),
                    ..Default::default()
                }
            } else {
                // Match bytes actually in the envelope (post --compress-br), not raw input.
                validate_inscription_reveal(&tx, body_ref)
            };
            println!("network: {}", network.as_str());
            println!("commit_address: {}", commit.address);
            println!("leaf_script_len: {}", commit.leaf_script.len());
            if let Some(d) = delegate_id {
                println!("delegate_id: {d}");
            }
            println!("reveal_txid_preview: {}", tx.compute_txid());
            println!("validation.allows_broadcast: {}", report.allows_broadcast());
            for e in &report.errors {
                println!("validation_error: {e}");
            }
            // postage/reveal_fee/commit_sats already printed by resolve_reveal_funding
            println!("dry-run complete (not broadcast)");
            return Ok(());
        }

        println!("network: {}", network.as_str());
        println!("commit_address: {}", commit.address);
        println!("leaf_script_len: {}", commit.leaf_script.len());
        if let Some(d) = delegate_id {
            println!("delegate_id: {d}");
        }
        // postage/reveal_fee/commit_sats already printed by resolve_reveal_funding
        println!("validation.allows_broadcast: true");
        println!("note: wallet must sign reveal (script-path) — Phechan has no commit private key");
        println!("dry-run complete (not broadcast)");
        return Ok(());
    }

    // Live: wallet-funded commit (--commit-txid) or bitcoind fund; grind vanity; keystore reveal.
    if !broadcast && !unsigned_psbt {
        return Err("internal: expected --broadcast or --unsigned-psbt for live path".into());
    }

    let dest_addr_str = flag_value(args, "--destination").ok_or(
        "--destination <address> required for live reveal (your ordinals / receive address)",
    )?;
    let dest = Address::from_str(&dest_addr_str)
        .map_err(|e| e.to_string())?
        .require_network(network.to_bitcoin())
        .map_err(|e| e.to_string())?;

    let (commit_txid, vout, value_sats) =
        if let Some(txid_s) = flag_value(args, "--commit-txid") {
            let rpc = BitcoindRpc::new(RpcConfig::from_env());
            if let (Some(vout_s), Some(val_s)) =
                (flag_value(args, "--commit-vout"), flag_value(args, "--commit-value"))
            {
                let vout: u32 = vout_s.parse().map_err(|_| "invalid --commit-vout")?;
                let value_sats: u64 = val_s.parse().map_err(|_| "invalid --commit-value")?;
                (txid_s, vout, value_sats)
            } else {
                // Local node often lacks sub-minrelay commits (sort-utxo uses Esplora).
                match rpc.get_raw_transaction_verbose(&txid_s) {
                    Ok(verbose) => {
                        let (vout, value_sats) = find_vout_for_address(
                            &verbose,
                            &commit.address.to_string(),
                        )
                        .ok_or_else(|| {
                            format!(
                                "commit output to {} not found in {}",
                                commit.address, txid_s
                            )
                        })?;
                        println!("commit_lookup_via: bitcoind");
                        (txid_s, vout, value_sats)
                    }
                    Err(rpc_err) => {
                        let esplora_tx = phechan_bitcoin::get_tx_json(network, &txid_s).map_err(
                            |e| format!("lookup commit tx: bitcoind ({rpc_err}); esplora ({e})"),
                        )?;
                        let (vout, value_sats) = phechan_bitcoin::find_vout_in_esplora_tx(
                            &esplora_tx,
                            &commit.address.to_string(),
                        )
                        .ok_or_else(|| {
                            format!(
                                "commit output to {} not found in {} (esplora)",
                                commit.address, txid_s
                            )
                        })?;
                        println!("commit_lookup_via: esplora");
                        (txid_s, vout, value_sats)
                    }
                }
            }
        } else {
            // Legacy: fund from local bitcoind wallet
            let rpc = BitcoindRpc::new(RpcConfig::from_env());
            let _ = rpc.get_block_count().map_err(|e| e.to_string())?;
            let btc = commit_value_sats as f64 / 100_000_000.0;
            let commit_txid = rpc
                .send_to_address(&commit.address.to_string(), btc)
                .map_err(|e| e.to_string())?;
            let verbose = rpc
                .get_raw_transaction_verbose(&commit_txid)
                .map_err(|e| e.to_string())?;
            let (vout, value_sats) = find_vout_for_address(&verbose, &commit.address.to_string())
                .ok_or("commit output not found in funding tx")?;
            (commit_txid, vout, value_sats)
        };

    if value_sats < postage_sats {
        return Err(format!(
            "funded commit {value_sats} sats < postage {postage_sats}"
        ));
    }
    let actual_fee = value_sats - postage_sats;
    let commit_txid_parsed = Txid::from_str(&commit_txid).map_err(|e| e.to_string())?;
    let commit_value = Amount::from_sat(value_sats);
    let dest_value = Amount::from_sat(postage_sats);

    println!("funded_commit_sats: {value_sats}");
    println!("postage_sats: {postage_sats}");
    println!("reveal_fee_sats: {actual_fee} (planned {fee_sats})");
    println!("destination: {dest_addr_str}");

    // Real parent-child FI/FO: spend parent as vin0. Tag-only --parent is refused
    // unless --same-sat-parent (parent already inside commit; single-input reveal).
    if let Some(parent_id_s) = parent_eff {
        if has_flag(args, "--same-sat-parent") {
            println!("parent_id: {parent_id_s}");
            println!("same_sat_parent: true");
            println!(
                "disclosure: provenance = tag 3 + reveal spends commit that holds the parent sat"
            );
            // Fall through to single-input reveal below.
        } else {
            let parent_out = flag_value(args, "--parent-outpoint").ok_or_else(|| {
                format!(
                    "parent {parent_id_s} set but --parent-outpoint missing — \
                     Verify parent in UI so we spend it (tag 3 alone is not provenance). \
                     For child on the parent's own sat, pass --same-sat-parent and fund commit from the parent UTXO."
                )
            })?;
            return reveal_parent_child_wallet(
                args,
                network,
                keystore.as_ref(),
                &commit,
                parent_id_s,
                &parent_out,
                &commit_txid,
                vout,
                value_sats,
                postage_sats,
                &dest,
                &dest_addr_str,
                unsigned_psbt,
                broadcast,
                body,
            );
        }
    }

    let mut psbt = build_reveal_psbt(RevealPsbtParams {
        commit_txid: commit_txid_parsed,
        commit_vout: vout,
        commit_value,
        commit_script_pubkey: commit.script_pubkey.clone(),
        destination_script_pubkey: dest.script_pubkey(),
        destination_value: dest_value,
        leaf_script: commit.leaf_script.clone(),
        spend_info: commit.spend_info.clone(),
        op_return: parse_op_return(args)?,
    })
    .map_err(|e| e.to_string())?;

    // Vanity grind on unsigned reveal (locktime), then re-sign.
    let vanity_prefix = flag_value(args, "--vanity-prefix").unwrap_or_default();
    let vanity_suffix = flag_value(args, "--vanity-suffix").unwrap_or_default();
    if !vanity_prefix.is_empty() || !vanity_suffix.is_empty() {
        let unsigned = psbt.unsigned_tx.clone();
        let max_tries: u32 = flag_value(args, "--vanity-max-tries")
            .unwrap_or_else(|| "5000000".into())
            .parse()
            .map_err(|_| "invalid --vanity-max-tries")?;
        let (tip_height, mediantime) = tip_for_vanity(network)?;
        println!(
            "vanity_grind: prefix={vanity_prefix:?} suffix={vanity_suffix:?} max_tries={max_tries} tip={tip_height} mediantime={mediantime}"
        );
        let (lt, ground_txid) = grind_locktime_affixes_final(
            &unsigned,
            &vanity_prefix,
            &vanity_suffix,
            tip_height,
            mediantime,
            max_tries,
        )
        .ok_or_else(|| {
            format!(
                "vanity grind failed within final locktime window (tip={tip_height}, mediantime={mediantime}, tries={max_tries}) for {vanity_prefix}…{vanity_suffix}"
            )
        })?;
        if !locktime_is_final(lt, tip_height, mediantime) {
            return Err(format!(
                "internal: ground locktime {lt} not final at tip={tip_height} mediantime={mediantime}"
            ));
        }
        psbt.unsigned_tx = with_lock_time(unsigned, lt);
        println!("vanity_lock_time: {lt}");
        println!("vanity_reveal_txid: {ground_txid}");
    }

    if unsigned_psbt {
        let b64 = base64::engine::general_purpose::STANDARD.encode(psbt.serialize());
        println!("network: {}", network.as_str());
        println!("commit_txid: {commit_txid}");
        println!("commit_vout: {vout}");
        println!("commit_address: {}", commit.address);
        println!("psbt_base64: {b64}");
        if keystore.is_none() {
            println!("note: wallet self-custody — sign this reveal PSBT in your wallet (script-path)");
            println!("note: only your wallet key can spend / recover this commit");
        } else {
            println!("note: reveal script-path spends the Phechan commit key (keystore), not Xverse");
            println!("note: prefer wallet sendTransfer → commit, then --broadcast with --commit-txid");
        }
        return Ok(());
    }

    let key = keystore.ok_or_else(|| {
        "internal: wallet custody should have returned unsigned PSBT before keystore sign".to_string()
    })?;
    let signed = sign_reveal_script_path(psbt, &key.keypair, &commit.leaf_script)
        .map_err(|e| e.to_string())?;
    let tx = finalize_to_tx(&signed).map_err(|e| e.to_string())?;
    if !body_ref.is_empty() {
        let report = validate_inscription_reveal(&tx, body_ref);
        if !report.allows_broadcast() {
            return Err(format!(
                "validation blocked reveal: {}",
                report.errors.join("; ")
            ));
        }
    }

    let reveal_hex = serialize_hex(&tx);
    let rpc = BitcoindRpc::new(RpcConfig::from_env());
    let reveal_txid = match network {
        Network::Regtest => rpc
            .send_raw_transaction(&reveal_hex)
            .map_err(|e| e.to_string())?,
        _ => match phechan_bitcoin::esplora_broadcast_tx(network, &reveal_hex) {
            Ok(id) => {
                println!("broadcast_via: esplora");
                id
            }
            Err(esplora_err) => match rpc.send_raw_transaction(&reveal_hex) {
                Ok(id) => {
                    println!("broadcast_via: bitcoind");
                    println!("note: esplora failed ({esplora_err}); used local node");
                    id
                }
                Err(rpc_err) => {
                    return Err(format!(
                        "reveal broadcast failed — esplora: {esplora_err}; bitcoind: {rpc_err}"
                    ));
                }
            },
        },
    };
    if network == Network::Regtest {
        let mine_to = rpc.get_new_address_bech32m().map_err(|e| e.to_string())?;
        rpc.generate_to_address(1, &mine_to)
            .map_err(|e| e.to_string())?;
        println!("mined: 1 block");
    } else {
        println!("note: wait for network confirmation (no local mine)");
    }

    println!("network: {}", network.as_str());
    println!("commit_txid: {commit_txid}");
    println!("commit_vout: {vout}");
    println!("commit_address: {}", commit.address);
    println!("reveal_txid: {reveal_txid}");
    println!("inscription_id_guess: {reveal_txid}i0");
    println!("reveal_hex: {reveal_hex}");
    Ok(())
}

/// FI/FO parent+child reveal for wallet UI (signet/testnet/regtest/mainnet).
///
/// Layout (always):
/// - vin0 = parent inscription UTXO (wallet signs)
/// - vin1 = commit (keystore script-path if automation key; else wallet signs too)
/// - vout0 = vault — parent sat returns here (same value as parent input)
/// - vout1 = child destination — new inscription postage
///
/// Returns a PSBT; wallet must sign remaining inputs, then finalize / broadcast.
fn reveal_parent_child_wallet(
    args: &[String],
    network: Network,
    keystore: Option<&RegtestKey>,
    commit: &CommitOutput,
    parent_id: &str,
    parent_out: &str,
    commit_txid_s: &str,
    commit_vout: u32,
    commit_value_sats: u64,
    postage_sats: u64,
    dest: &Address,
    dest_addr_str: &str,
    unsigned_psbt: bool,
    broadcast: bool,
    body: &[u8],
) -> Result<(), String> {
    let _body = body; // envelope already baked into commit leaf
    let (parent_txid_s, parent_vout) = parse_outpoint(parent_out)?;
    let parent_txid = Txid::from_str(&parent_txid_s).map_err(|e| e.to_string())?;
    let commit_txid = Txid::from_str(commit_txid_s).map_err(|e| e.to_string())?;

    let parent_value_sats: u64 = flag_value(args, "--parent-value")
        .ok_or("--parent-value <sats> required with --parent-outpoint")?
        .parse()
        .map_err(|_| "invalid --parent-value")?;

    let vault_addr_s = flag_value(args, "--vault-address")
        .ok_or("--vault-address required (where parent lands after reveal — usually your ordinals address)")?;
    let vault = Address::from_str(&vault_addr_s)
        .map_err(|e| e.to_string())?
        .require_network(network.to_bitcoin())
        .map_err(|e| e.to_string())?;

    // Parent scriptPubKey: explicit hex, else from address, else lookup tx.
    let parent_spk = if let Some(hex_s) = flag_value(args, "--parent-script-hex") {
        let raw = hex::decode(hex_s.trim()).map_err(|e| format!("parent-script-hex: {e}"))?;
        ScriptBuf::from_bytes(raw)
    } else if let Some(addr_s) = flag_value(args, "--parent-address") {
        Address::from_str(&addr_s)
            .map_err(|e| e.to_string())?
            .require_network(network.to_bitcoin())
            .map_err(|e| e.to_string())?
            .script_pubkey()
    } else {
        // Esplora / bitcoind lookup
        let rpc = BitcoindRpc::new(RpcConfig::from_env());
        match rpc.get_raw_transaction_verbose(&parent_txid_s) {
            Ok(verbose) => {
                vout_value_and_spk(&verbose, parent_vout)
                    .ok_or("parent vout not found")?
                    .1
            }
            Err(_) => {
                let esplora_tx = phechan_bitcoin::get_tx_json(network, &parent_txid_s)
                    .map_err(|e| format!("lookup parent tx: {e}"))?;
                let vouts = esplora_tx
                    .get("vout")
                    .and_then(|v| v.as_array())
                    .ok_or("parent tx missing vout")?;
                let out = vouts
                    .get(parent_vout as usize)
                    .ok_or("parent vout OOB")?;
                let spk_hex = out
                    .get("scriptpubkey")
                    .or_else(|| out.get("scriptPubKey").and_then(|s| s.get("hex")))
                    .and_then(|v| v.as_str())
                    .ok_or("parent scriptpubkey missing")?;
                ScriptBuf::from_bytes(hex::decode(spk_hex).map_err(|e| e.to_string())?)
            }
        }
    };

    let parent_tap_key = flag_value(args, "--ordinals-pubkey-hex")
        .map(|pk| parse_xonly_pubkey(&pk))
        .transpose()?;

    if !parent_spk.is_p2tr() {
        return Err(
            "parent UTXO must be P2TR (ordinals address). Nested/legacy parents are not supported in UI yet."
                .into(),
        );
    }
    if parent_tap_key.is_none() {
        println!(
            "warning: no --ordinals-pubkey-hex — Xverse may refuse to sign parent without tapInternalKey"
        );
    }

    let fee_sats = commit_value_sats.saturating_sub(postage_sats);
    let child_value = Amount::from_sat(postage_sats);
    if child_value.to_sat() < P2TR_DUST_SATS {
        return Err(format!(
            "child postage {postage_sats} below dust {P2TR_DUST_SATS}"
        ));
    }

    let live_layout = validate_parent_child_layout(ParentChildValidationInput {
        policy: ParentPlacementPolicy::FirstInFirstOut,
        parent_input_index: 0,
        parent_sat_offset: 0,
        vault_vout: 0,
        input_values: &[parent_value_sats, commit_value_sats],
        output_values: &[parent_value_sats, postage_sats],
        labeled_inputs: &[LabeledUtxo {
            txid_hex: parent_txid_s.clone(),
            vout: parent_vout,
            value: parent_value_sats,
            hint: UtxoAssetHint::Inscription,
            inscription_ids: vec![parent_id.to_string()],
            rune_summary: None,
        }],
        has_validated_runestone: false,
        allow_asset_bearing_fees: true,
    });
    for d in &live_layout.asset_disclosure {
        println!("disclosure: {d}");
    }
    for e in &live_layout.errors {
        println!("error: {e}");
    }
    if !live_layout.allows_broadcast() {
        return Err(format!(
            "parent/child layout blocked: {}",
            live_layout.errors.join("; ")
        ));
    }

    let mut psbt = build_parent_child_reveal_psbt(ParentChildRevealParams {
        parent_txid,
        parent_vout,
        parent_value: Amount::from_sat(parent_value_sats),
        parent_script_pubkey: parent_spk,
        parent_tap_internal_key: parent_tap_key,
        commit_txid,
        commit_vout,
        commit_value: Amount::from_sat(commit_value_sats),
        commit_script_pubkey: commit.script_pubkey.clone(),
        vault_script_pubkey: vault.script_pubkey(),
        vault_value: Amount::from_sat(parent_value_sats),
        child_script_pubkey: dest.script_pubkey(),
        child_value,
        leaf_script: commit.leaf_script.clone(),
        spend_info: commit.spend_info.clone(),
    })
    .map_err(|e| e.to_string())?;

    // Vanity grind (both inputs empty scriptSig — TXID stable through parent witness).
    let vanity_prefix = flag_value(args, "--vanity-prefix").unwrap_or_default();
    let vanity_suffix = flag_value(args, "--vanity-suffix").unwrap_or_default();
    if !vanity_prefix.is_empty() || !vanity_suffix.is_empty() {
        let template = txid_grind_template(&psbt).map_err(|e| e.to_string())?;
        let max_tries: u32 = flag_value(args, "--vanity-max-tries")
            .unwrap_or_else(|| "5000000".into())
            .parse()
            .map_err(|_| "invalid --vanity-max-tries")?;
        let (tip_height, mediantime) = tip_for_vanity(network)?;
        let (lt, ground_txid) = grind_locktime_affixes_final(
            &template,
            &vanity_prefix,
            &vanity_suffix,
            tip_height,
            mediantime,
            max_tries,
        )
        .ok_or_else(|| {
            format!(
                "vanity grind failed within final locktime window for {vanity_prefix}…{vanity_suffix}"
            )
        })?;
        psbt.unsigned_tx = with_lock_time(psbt.unsigned_tx.clone(), lt);
        println!("vanity_lock_time: {lt}");
        println!("vanity_reveal_txid: {ground_txid}");
    }

    // Sign commit (input 1) only when we have a local keystore key.
    // Wallet custody: leave both inputs for the wallet (parent key-path + commit script-path).
    let out_psbt = if let Some(key) = keystore {
        sign_reveal_script_path_at(psbt, 1, &key.keypair, &commit.leaf_script)
            .map_err(|e| e.to_string())?
    } else {
        psbt
    };

    println!("network: {}", network.as_str());
    println!("parent_id: {parent_id}");
    println!("parent_outpoint: {parent_out}");
    println!("parent_value_sats: {parent_value_sats}");
    println!("vault_address: {vault_addr_s}");
    println!("vault_vout: 0");
    println!(
        "parent_lands: vout0 {vault_addr_s} ({parent_value_sats} sats) — same sat(s) as parent, returned after spend"
    );
    println!(
        "child_lands: vout1 {dest_addr_str} ({postage_sats} sats postage) — new child inscription"
    );
    println!("reveal_fee_sats: {fee_sats}");
    println!("commit_txid: {commit_txid_s}");
    println!("placement: FirstInFirstOut");
    if keystore.is_some() {
        println!("sign_inputs_hint: wallet must sign input 0 (ordinals / parent address)");
    } else {
        println!(
            "sign_inputs_hint: wallet must sign input 0 (parent) and input 1 (commit script-path)"
        );
    }

    let b64 = base64::engine::general_purpose::STANDARD.encode(out_psbt.serialize());
    println!("psbt_base64: {b64}");
    if keystore.is_some() {
        println!("note: commit input is keystore-signed; sign parent in wallet, then finalize+broadcast");
    } else {
        println!("note: wallet self-custody — sign parent + commit in wallet; Phechan holds no reveal key");
    }

    if unsigned_psbt || !broadcast {
        println!("parent_child_psbt: awaiting wallet signature");
        return Ok(());
    }

    // Full auto-broadcast only when parent already signed (regtest bitcoind wallet path).
    // Signet UI always uses unsigned return + wallet sign.
    Err(
        "parent-child on signet/testnet: take psbt_base64, sign input 0 in wallet, finalize via UI (do not pass --broadcast alone)"
            .into(),
    )
}

fn parse_xonly_pubkey(hex_s: &str) -> Result<XOnlyPublicKey, String> {
    let mut raw = hex::decode(hex_s.trim()).map_err(|e| format!("ordinals-pubkey-hex: {e}"))?;
    if raw.len() == 33 && (raw[0] == 0x02 || raw[0] == 0x03) {
        raw = raw[1..].to_vec();
    }
    if raw.len() != 32 {
        return Err(format!(
            "ordinals-pubkey-hex: expected 32-byte x-only (or 33 compressed), got {}",
            raw.len()
        ));
    }
    let arr: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| "ordinals-pubkey-hex: bad length")?;
    XOnlyPublicKey::from_slice(&arr).map_err(|e| format!("ordinals-pubkey-hex: {e}"))
}

fn child(args: &[String]) -> Result<(), String> {
    let body = flag_value(args, "--body").unwrap_or_else(|| "child".to_string());
    let parent_id = flag_value(args, "--parent").ok_or("--parent <inscription_id> required")?;
    let network =
        Network::parse(&flag_value(args, "--network").unwrap_or_else(|| "regtest".into()))
            .ok_or("invalid --network")?;
    let dry_run = has_flag(args, "--dry-run");
    let broadcast = has_flag(args, "--broadcast");
    if !dry_run && !broadcast {
        return Err("require --dry-run or --broadcast".into());
    }
    if broadcast {
        require_mainnet_broadcast_gate(args, network)?;
    }
    if broadcast
        && network != Network::Regtest
        && network != Network::Signet
        && network != Network::Testnet
        && network != Network::Mainnet
    {
        return Err("child --broadcast supports regtest|signet|testnet|mainnet".into());
    }

    let placement = match flag_value(args, "--placement")
        .unwrap_or_else(|| "fifo".into())
        .as_str()
    {
        "fifo" => ParentPlacementPolicy::FirstInFirstOut,
        "custom" => ParentPlacementPolicy::Custom,
        other => return Err(format!("unknown --placement {other}")),
    };
    if broadcast && !matches!(placement, ParentPlacementPolicy::FirstInFirstOut) {
        return Err("live child --broadcast currently supports --placement fifo only".into());
    }

    let parent_value: u64 = flag_value(args, "--parent-value")
        .unwrap_or_else(|| "1000".into())
        .parse()
        .map_err(|_| "invalid --parent-value")?;
    let funding_value: u64 = flag_value(args, "--funding-value")
        .unwrap_or_else(|| "5000".into())
        .parse()
        .map_err(|_| "invalid --funding-value")?;
    let parent_has_runes = has_flag(args, "--parent-has-runes");
    let allow_asset_fees = has_flag(args, "--allow-asset-fees");

    let (parent_input_index, vault_vout, input_values, output_values) = match placement {
        ParentPlacementPolicy::FirstInFirstOut => {
            let fee = 200u64;
            (
                0usize,
                0usize,
                vec![parent_value, funding_value],
                vec![parent_value, funding_value.saturating_sub(fee)],
            )
        }
        ParentPlacementPolicy::Custom => {
            let fee = 200u64;
            (
                1usize,
                1usize,
                vec![funding_value, parent_value],
                vec![funding_value.saturating_sub(fee), parent_value],
            )
        }
    };

    let hint = if parent_has_runes {
        UtxoAssetHint::InscriptionAndRune
    } else {
        UtxoAssetHint::Inscription
    };
    let labeled = [LabeledUtxo {
        txid_hex: "00".repeat(32),
        vout: 0,
        value: parent_value,
        hint,
        inscription_ids: vec![parent_id.clone()],
        rune_summary: parent_has_runes.then(|| "EXAMPLE•RUNE".into()),
    }];

    let layout_report = validate_parent_child_layout(ParentChildValidationInput {
        policy: placement,
        parent_input_index,
        parent_sat_offset: 0,
        vault_vout,
        input_values: &input_values,
        output_values: &output_values,
        labeled_inputs: &labeled,
        has_validated_runestone: false,
        allow_asset_bearing_fees: allow_asset_fees,
    });

    let key = derive_regtest_key(network, "inscription-child").map_err(|e| e.to_string())?;
    let xonly = key.xonly.serialize();
    let leaf = build_inscription_tapscript_with_parent(&xonly, body.as_bytes(), Some(&parent_id))
        .map_err(|e| e.to_string())?;
    let commit = build_commit_output(network, &xonly, leaf).map_err(|e| e.to_string())?;

    println!("network: {}", network.as_str());
    println!("placement: {:?}", placement);
    println!("parent_id: {parent_id}");
    println!("commit_address: {}", commit.address);
    println!(
        "layout_validation.allows_broadcast: {}",
        layout_report.allows_broadcast()
    );
    for d in &layout_report.asset_disclosure {
        println!("disclosure: {d}");
    }
    for e in &layout_report.errors {
        println!("error: {e}");
    }
    if !layout_report.allows_broadcast() {
        return Err("parent/asset layout validation failed".into());
    }

    if dry_run && !broadcast {
        println!("dry-run complete");
        return Ok(());
    }

    // Live FI/FO: spend wallet parent outpoint + keystore commit.
    let parent_out = flag_value(args, "--parent-outpoint")
        .ok_or("--parent-outpoint <txid:vout> required for --broadcast")?;
    let (parent_txid_s, parent_vout) = parse_outpoint(&parent_out)?;
    let fee_sats: u64 = flag_value(args, "--fee-sats")
        .unwrap_or_else(|| "500".into())
        .parse()
        .map_err(|_| "invalid --fee-sats")?;
    let commit_sats: u64 = flag_value(args, "--commit-sats")
        .unwrap_or_else(|| "10000".into())
        .parse()
        .map_err(|_| "invalid --commit-sats")?;
    if commit_sats <= fee_sats + 330 {
        return Err("commit value too small for fee + dust".into());
    }

    let rpc = BitcoindRpc::new(RpcConfig::from_env());
    let parent_verbose = rpc
        .get_raw_transaction_verbose(&parent_txid_s)
        .map_err(|e| e.to_string())?;
    let (parent_value_sats, parent_spk) =
        vout_value_and_spk(&parent_verbose, parent_vout).ok_or("parent vout not found")?;
    let parent_txid = Txid::from_str(&parent_txid_s).map_err(|e| e.to_string())?;

    let btc = commit_sats as f64 / 100_000_000.0;
    let commit_txid_s = rpc
        .send_to_address(&commit.address.to_string(), btc)
        .map_err(|e| e.to_string())?;
    let commit_verbose = rpc
        .get_raw_transaction_verbose(&commit_txid_s)
        .map_err(|e| e.to_string())?;
    let (commit_vout, commit_value_sats) =
        find_vout_for_address(&commit_verbose, &commit.address.to_string())
            .ok_or("commit output not found")?;
    let commit_txid = Txid::from_str(&commit_txid_s).map_err(|e| e.to_string())?;

    let vault_addr = rpc.get_new_address_bech32m().map_err(|e| e.to_string())?;
    let vault = Address::from_str(&vault_addr)
        .map_err(|e| e.to_string())?
        .require_network(network.to_bitcoin())
        .map_err(|e| e.to_string())?;
    let child_addr = rpc.get_new_address_bech32m().map_err(|e| e.to_string())?;
    let child_dest = Address::from_str(&child_addr)
        .map_err(|e| e.to_string())?
        .require_network(network.to_bitcoin())
        .map_err(|e| e.to_string())?;

    let child_value = Amount::from_sat(commit_value_sats.saturating_sub(fee_sats));
    let live_layout = validate_parent_child_layout(ParentChildValidationInput {
        policy: ParentPlacementPolicy::FirstInFirstOut,
        parent_input_index: 0,
        parent_sat_offset: 0,
        vault_vout: 0,
        input_values: &[parent_value_sats, commit_value_sats],
        output_values: &[parent_value_sats, child_value.to_sat()],
        labeled_inputs: &[LabeledUtxo {
            txid_hex: parent_txid_s.clone(),
            vout: parent_vout,
            value: parent_value_sats,
            hint: UtxoAssetHint::Inscription,
            inscription_ids: vec![parent_id.clone()],
            rune_summary: None,
        }],
        has_validated_runestone: false,
        allow_asset_bearing_fees: true,
    });
    if !live_layout.allows_broadcast() {
        return Err(format!(
            "live layout blocked: {}",
            live_layout.errors.join("; ")
        ));
    }

    let psbt = build_parent_child_reveal_psbt(ParentChildRevealParams {
        parent_txid,
        parent_vout,
        parent_value: Amount::from_sat(parent_value_sats),
        parent_script_pubkey: parent_spk,
        parent_tap_internal_key: None,
        commit_txid,
        commit_vout,
        commit_value: Amount::from_sat(commit_value_sats),
        commit_script_pubkey: commit.script_pubkey.clone(),
        vault_script_pubkey: vault.script_pubkey(),
        vault_value: Amount::from_sat(parent_value_sats),
        child_script_pubkey: child_dest.script_pubkey(),
        child_value,
        leaf_script: commit.leaf_script.clone(),
        spend_info: commit.spend_info.clone(),
    })
    .map_err(|e| e.to_string())?;

    // Sign commit (input 1) with keystore; wallet signs parent (input 0).
    let partially = sign_reveal_script_path_at(psbt, 1, &key.keypair, &commit.leaf_script)
        .map_err(|e| e.to_string())?;
    let psbt_b64 = base64::engine::general_purpose::STANDARD.encode(partially.serialize());
    let processed = rpc
        .wallet_process_psbt(&psbt_b64)
        .map_err(|e| e.to_string())?;
    let signed_b64 = processed
        .get("psbt")
        .and_then(|v| v.as_str())
        .ok_or("walletprocesspsbt missing psbt")?;
    let finalized = rpc.finalize_psbt(signed_b64).map_err(|e| e.to_string())?;
    let hex = if let Some(h) = finalized.get("hex").and_then(|v| v.as_str()) {
        h.to_string()
    } else {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(signed_b64.trim())
            .map_err(|e| e.to_string())?;
        let merged = Psbt::deserialize(&bytes).map_err(|e| e.to_string())?;
        let tx = finalize_to_tx(&merged).map_err(|e| {
            format!("finalize incomplete (wallet may not own parent): {e}; result={finalized}")
        })?;
        serialize_hex(&tx)
    };

    let reveal_txid = rpc
        .send_raw_transaction(&hex)
        .map_err(|e| e.to_string())?;
    let mine_to = rpc.get_new_address_bech32m().map_err(|e| e.to_string())?;
    rpc.generate_to_address(1, &mine_to)
        .map_err(|e| e.to_string())?;

    println!("commit_txid: {commit_txid_s}");
    println!("parent_outpoint: {parent_out}");
    println!("vault_address: {vault_addr}");
    println!("reveal_txid: {reveal_txid}");
    println!("mined: 1 block");
    println!("child_inscription_id_guess: {reveal_txid}i0");
    Ok(())
}

fn inspect(args: &[String]) -> Result<(), String> {
    let body = flag_value(args, "--body");
    let hex = flag_value(args, "--envelope-hex");
    if let Some(b) = body {
        let env = build_text_envelope(b.as_bytes());
        println!("envelope_len: {}", env.len());
        println!("envelope_hex: {}", hex::encode(&env));
        println!("contains_ord: {}", contains_slice(&env, b"ord"));
        return Ok(());
    }
    if let Some(h) = hex {
        let bytes = hex::decode(h.trim()).map_err(|e| e.to_string())?;
        println!("envelope_len: {}", bytes.len());
        println!("contains_ord: {}", contains_slice(&bytes, b"ord"));
        if let Some(ct) = find_ascii_after(&bytes, b"text/") {
            println!("content_type_guess: text/{ct}");
        }
        return Ok(());
    }
    Err("inscription inspect requires --body <text> or --envelope-hex <hex>".into())
}

/// Recover a stranded commit by reusing the tapscript leaf from a prior reveal
/// that spent the *same* commit address (same leaf + keystore key).
///
/// Example: commit A and commit B both pay tb1p…X; reveal of A has the leaf in
/// its witness → recover-reveal can spend B without regenerating form fields.
fn recover_reveal(args: &[String]) -> Result<(), String> {
    let network =
        Network::parse(&flag_value(args, "--network").unwrap_or_else(|| "signet".into()))
            .ok_or("invalid --network")?;
    if has_flag(args, "--broadcast") {
        require_mainnet_broadcast_gate(args, network)?;
    }
    let commit_txid_s =
        flag_value(args, "--commit-txid").ok_or("--commit-txid <txid> required")?;
    let dest_s = flag_value(args, "--destination")
        .ok_or("--destination <ordinals address> required")?;
    let dest = Address::from_str(&dest_s)
        .map_err(|e| e.to_string())?
        .require_network(network.to_bitcoin())
        .map_err(|e| e.to_string())?;

    let leaf_bytes = if let Some(h) = flag_value(args, "--leaf-hex") {
        hex::decode(h.trim()).map_err(|e| format!("leaf-hex: {e}"))?
    } else {
        let from = flag_value(args, "--from-reveal-txid")
            .ok_or("--leaf-hex <hex> or --from-reveal-txid <txid> required")?;
        let tx = phechan_bitcoin::get_tx_json(network, &from)
            .map_err(|e| format!("fetch prior reveal: {e}"))?;
        extract_tapscript_leaf_from_esplora_tx(&tx)?
    };
    println!("leaf_script_len: {}", leaf_bytes.len());

    let key = derive_regtest_key(network, "inscription-create").map_err(|e| e.to_string())?;
    let xonly = key.xonly.serialize();
    // Leaf must start with push(xonly) OP_CHECKSIG for this keystore key.
    if leaf_bytes.len() < 34 || leaf_bytes[0] != 0x20 || leaf_bytes[33] != 0xac {
        return Err("leaf does not look like <xonly> OP_CHECKSIG + envelope".into());
    }
    if leaf_bytes[1..33] != xonly {
        return Err(
            "leaf internal key does not match local inscription-create keystore — cannot sign"
                .into(),
        );
    }

    let commit = build_commit_output(network, &xonly, leaf_bytes).map_err(|e| e.to_string())?;
    println!("recovered_commit_address: {}", commit.address);

    let postage_sats = parse_postage(args)?;
    let commit_txid = Txid::from_str(&commit_txid_s).map_err(|e| e.to_string())?;

    let (vout, value_sats) = if let (Some(vout_s), Some(val_s)) = (
        flag_value(args, "--commit-vout"),
        flag_value(args, "--commit-value"),
    ) {
        (
            vout_s.parse::<u32>().map_err(|_| "invalid --commit-vout")?,
            val_s.parse::<u64>().map_err(|_| "invalid --commit-value")?,
        )
    } else {
        let esplora_tx = phechan_bitcoin::get_tx_json(network, &commit_txid_s)
            .map_err(|e| format!("lookup commit: {e}"))?;
        phechan_bitcoin::find_vout_in_esplora_tx(&esplora_tx, &commit.address.to_string())
            .ok_or_else(|| {
                format!(
                    "commit output to {} not found in {commit_txid_s} — leaf/address mismatch",
                    commit.address
                )
            })?
    };

    if value_sats < postage_sats {
        return Err(format!(
            "commit {value_sats} sats < postage {postage_sats}"
        ));
    }
    println!("commit_vout: {vout}");
    println!("funded_commit_sats: {value_sats}");
    println!("postage_sats: {postage_sats}");
    println!("reveal_fee_sats: {}", value_sats - postage_sats);

    let mut psbt = build_reveal_psbt(RevealPsbtParams {
        commit_txid,
        commit_vout: vout,
        commit_value: Amount::from_sat(value_sats),
        commit_script_pubkey: commit.script_pubkey.clone(),
        destination_script_pubkey: dest.script_pubkey(),
        destination_value: Amount::from_sat(postage_sats),
        leaf_script: commit.leaf_script.clone(),
        spend_info: commit.spend_info.clone(),
        op_return: parse_op_return(args)?,
    })
    .map_err(|e| e.to_string())?;

    let vanity_prefix = flag_value(args, "--vanity-prefix").unwrap_or_default();
    let vanity_suffix = flag_value(args, "--vanity-suffix").unwrap_or_default();
    if !vanity_prefix.is_empty() || !vanity_suffix.is_empty() {
        let unsigned = psbt.unsigned_tx.clone();
        let max_tries: u32 = flag_value(args, "--vanity-max-tries")
            .unwrap_or_else(|| "5000000".into())
            .parse()
            .map_err(|_| "invalid --vanity-max-tries")?;
        let (tip_height, mediantime) = tip_for_vanity(network)?;
        let (lt, ground_txid) = grind_locktime_affixes_final(
            &unsigned,
            &vanity_prefix,
            &vanity_suffix,
            tip_height,
            mediantime,
            max_tries,
        )
        .ok_or_else(|| {
            format!("vanity grind failed for {vanity_prefix}…{vanity_suffix}")
        })?;
        psbt.unsigned_tx = with_lock_time(unsigned, lt);
        println!("vanity_lock_time: {lt}");
        println!("vanity_reveal_txid: {ground_txid}");
    }

    if has_flag(args, "--dry-run") {
        let signed = sign_reveal_script_path(psbt, &key.keypair, &commit.leaf_script)
            .map_err(|e| e.to_string())?;
        let tx = finalize_to_tx(&signed).map_err(|e| e.to_string())?;
        println!("reveal_txid_preview: {}", tx.compute_txid());
        println!("dry-run complete (not broadcast)");
        return Ok(());
    }
    if !has_flag(args, "--broadcast") {
        return Err("pass --broadcast or --dry-run".into());
    }

    let signed = sign_reveal_script_path(psbt, &key.keypair, &commit.leaf_script)
        .map_err(|e| e.to_string())?;
    let tx = finalize_to_tx(&signed).map_err(|e| e.to_string())?;
    let reveal_hex = serialize_hex(&tx);
    let reveal_txid = match phechan_bitcoin::esplora_broadcast_tx(network, &reveal_hex) {
        Ok(id) => {
            println!("broadcast_via: esplora");
            id
        }
        Err(esplora_err) => {
            let rpc = BitcoindRpc::new(RpcConfig::from_env());
            match rpc.send_raw_transaction(&reveal_hex) {
                Ok(id) => {
                    println!("broadcast_via: bitcoind");
                    println!("note: esplora failed ({esplora_err})");
                    id
                }
                Err(rpc_err) => {
                    return Err(format!(
                        "broadcast failed — esplora: {esplora_err}; bitcoind: {rpc_err}"
                    ));
                }
            }
        }
    };
    println!("commit_txid: {commit_txid_s}");
    println!("reveal_txid: {reveal_txid}");
    println!("inscription_id_guess: {reveal_txid}i0");
    println!("destination: {dest_s}");
    Ok(())
}

fn extract_tapscript_leaf_from_esplora_tx(tx: &serde_json::Value) -> Result<Vec<u8>, String> {
    let vins = tx
        .get("vin")
        .and_then(|v| v.as_array())
        .ok_or("prior reveal missing vin")?;
    let wit = vins
        .first()
        .and_then(|v| v.get("witness"))
        .and_then(|v| v.as_array())
        .ok_or("prior reveal missing witness")?;
    // Script-path: [schnorr_sig(64), script, control_block]
    if wit.len() < 2 {
        return Err(format!(
            "prior reveal witness too short ({} items)",
            wit.len()
        ));
    }
    let script_hex = wit[1]
        .as_str()
        .ok_or("witness[1] not a hex string")?;
    hex::decode(script_hex).map_err(|e| format!("decode leaf: {e}"))
}

fn parse_outpoint(s: &str) -> Result<(String, u32), String> {
    let (txid, vout_s) = s
        .split_once(':')
        .ok_or_else(|| format!("outpoint must be txid:vout, got {s}"))?;
    if txid.len() != 64 {
        return Err("txid must be 64 hex chars".into());
    }
    let vout: u32 = vout_s.parse().map_err(|_| "bad vout")?;
    Ok((txid.to_string(), vout))
}

fn vout_value_and_spk(verbose_tx: &serde_json::Value, vout: u32) -> Option<(u64, ScriptBuf)> {
    let vouts = verbose_tx.get("vout")?.as_array()?;
    let out = vouts
        .iter()
        .find(|v| v.get("n").and_then(|n| n.as_u64()) == Some(vout as u64))?;
    let value_btc = out.get("value")?.as_f64()?;
    let sats = (value_btc * 100_000_000.0).round() as u64;
    let hex = out
        .get("scriptPubKey")?
        .get("hex")?
        .as_str()?;
    let bytes = hex::decode(hex).ok()?;
    Some((sats, ScriptBuf::from_bytes(bytes)))
}

fn contains_slice(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn find_ascii_after(hay: &[u8], prefix: &[u8]) -> Option<String> {
    for w in hay.windows(prefix.len() + 8) {
        if w.starts_with(prefix) {
            let s = String::from_utf8_lossy(w);
            let end = s
                .find(|c: char| c == ';' || c == '"' || c < ' ')
                .unwrap_or(s.len().min(32));
            return Some(s[..end].trim_start_matches("text/").to_string());
        }
    }
    None
}
