//! Inscription tapscript leaf: <xonly> OP_CHECKSIG + envelope.

use crate::envelope::{
    build_delegate_envelope, build_text_envelope, build_text_envelope_with_parent, EnvelopeOptions,
    build_envelope,
};
use crate::parent_id::InscriptionIdError;
use crate::script_push::{compile_script, ScriptChunk, OP_CHECKSIG};

/// Build tapscript: 32-byte x-only pubkey, OP_CHECKSIG, then inscription envelope opcodes.
pub fn build_inscription_tapscript(internal_xonly: &[u8; 32], body: &[u8]) -> Vec<u8> {
    build_inscription_tapscript_with_parent(internal_xonly, body, None)
        .expect("envelope without parent cannot fail")
}

/// Build tapscript with optional parent tag in the envelope.
pub fn build_inscription_tapscript_with_parent(
    internal_xonly: &[u8; 32],
    body: &[u8],
    parent_id: Option<&str>,
) -> Result<Vec<u8>, InscriptionIdError> {
    let prefix = key_prefix(internal_xonly);
    let envelope = match parent_id {
        Some(pid) => build_text_envelope_with_parent(body, Some(pid))?,
        None => build_text_envelope(body),
    };
    Ok(concat_scripts(&prefix, &envelope))
}

/// Build tapscript whose envelope delegates content to `delegate_id`.
pub fn build_delegate_tapscript(
    internal_xonly: &[u8; 32],
    delegate_id: &str,
    body: Option<&[u8]>,
) -> Result<Vec<u8>, InscriptionIdError> {
    let prefix = key_prefix(internal_xonly);
    let envelope = build_delegate_envelope(delegate_id, body)?;
    Ok(concat_scripts(&prefix, &envelope))
}

/// Build tapscript with arbitrary envelope options (parent and/or delegate).
pub fn build_inscription_tapscript_opts(
    internal_xonly: &[u8; 32],
    content_type: &[u8],
    body: &[u8],
    opts: EnvelopeOptions<'_>,
) -> Result<Vec<u8>, InscriptionIdError> {
    let prefix = key_prefix(internal_xonly);
    let envelope = build_envelope(content_type, body, opts)?;
    Ok(concat_scripts(&prefix, &envelope))
}

fn key_prefix(internal_xonly: &[u8; 32]) -> Vec<u8> {
    compile_script(&[
        ScriptChunk::Data(internal_xonly.to_vec()),
        ScriptChunk::Op(OP_CHECKSIG),
    ])
}

fn concat_scripts(prefix: &[u8], envelope: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + envelope.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(envelope);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tapscript_starts_with_32byte_key_and_checksig() {
        let key = [0x02u8; 32];
        let script = build_inscription_tapscript(&key, b"hi");
        assert_eq!(script[0], 0x20); // OP_PUSHBYTES_32
        assert_eq!(&script[1..33], &key);
        assert_eq!(script[33], 0xac); // OP_CHECKSIG
        assert_eq!(script[34], 0x00); // OP_FALSE of envelope
    }

    #[test]
    fn delegate_tapscript_contains_ord_and_tag11() {
        let key = [0x03u8; 32];
        let id = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi0";
        let script = build_delegate_tapscript(&key, id, None).unwrap();
        assert!(script.windows(3).any(|w| w == b"ord"));
        assert!(script.contains(&0x5b));
    }
}
