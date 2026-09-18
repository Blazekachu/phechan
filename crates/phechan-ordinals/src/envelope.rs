//! Ordinals inscription envelope encoding.
//! Reference: https://docs.ordinals.com/inscriptions.html

use crate::parent_id::{encode_inscription_id, InscriptionIdError};
use crate::script_push::{
    compile_script, ScriptChunk, MAX_PUSH_SIZE, OP_ENDIF, OP_FALSE, OP_IF,
};

const TAG_BODY: u8 = 0;
const TAG_CONTENT_TYPE: u8 = 1;
const TAG_PARENT: u8 = 3;
const TAG_METADATA: u8 = 5;
const TAG_METAPROTOCOL: u8 = 7;
const TAG_CONTENT_ENCODING: u8 = 9;
const TAG_DELEGATE: u8 = 11;
const TAG_PROPERTIES: u8 = 17;

/// Options for building an inscription envelope.
#[derive(Debug, Clone, Default)]
pub struct EnvelopeOptions<'a> {
    pub parent_id: Option<&'a str>,
    pub delegate_id: Option<&'a str>,
    pub metaprotocol: Option<&'a str>,
    /// Raw tag-5 metadata bytes (ideally CBOR). Independent of title.
    pub metadata: Option<&'a [u8]>,
    /// Display title → Properties (tag 17) Attributes key 0. Not metadata.
    pub title: Option<&'a str>,
    pub content_encoding: Option<&'a [u8]>,
    /// When true, omit content-type and body (delegate-only pointer).
    pub omit_content: bool,
}

/// Build a text/plain inscription envelope for `body`.
pub fn build_text_envelope(body: &[u8]) -> Vec<u8> {
    build_envelope(b"text/plain;charset=utf-8", body, EnvelopeOptions::default())
        .expect("no parent/delegate")
}

/// Build a text envelope with optional parent inscription id.
pub fn build_text_envelope_with_parent(
    body: &[u8],
    parent_id: Option<&str>,
) -> Result<Vec<u8>, InscriptionIdError> {
    build_envelope(
        b"text/plain;charset=utf-8",
        body,
        EnvelopeOptions {
            parent_id,
            ..Default::default()
        },
    )
}

/// Build a delegate envelope (tag 11). Body/content-type optional.
pub fn build_delegate_envelope(
    delegate_id: &str,
    body: Option<&[u8]>,
) -> Result<Vec<u8>, InscriptionIdError> {
    match body {
        Some(b) if !b.is_empty() => build_envelope(
            b"text/plain;charset=utf-8",
            b,
            EnvelopeOptions {
                delegate_id: Some(delegate_id),
                ..Default::default()
            },
        ),
        _ => build_envelope(
            b"",
            b"",
            EnvelopeOptions {
                delegate_id: Some(delegate_id),
                omit_content: true,
                ..Default::default()
            },
        ),
    }
}

/// Build an inscription envelope with the given content type and body.
pub fn build_envelope(
    content_type: &[u8],
    body: &[u8],
    opts: EnvelopeOptions<'_>,
) -> Result<Vec<u8>, InscriptionIdError> {
    let mut chunks: Vec<ScriptChunk> = Vec::new();
    chunks.push(ScriptChunk::Op(OP_FALSE));
    chunks.push(ScriptChunk::Op(OP_IF));
    chunks.push(ScriptChunk::Data(b"ord".to_vec()));

    if !opts.omit_content {
        chunks.push(ScriptChunk::Data(vec![TAG_CONTENT_TYPE]));
        chunks.push(ScriptChunk::Data(content_type.to_vec()));
    }

    if let Some(pid) = opts.parent_id {
        let parent_bytes = encode_inscription_id(pid)?;
        chunks.push(ScriptChunk::Data(vec![TAG_PARENT]));
        chunks.push(ScriptChunk::Data(parent_bytes));
    }

    if let Some(did) = opts.delegate_id {
        let delegate_bytes = encode_inscription_id(did)?;
        chunks.push(ScriptChunk::Data(vec![TAG_DELEGATE]));
        chunks.push(ScriptChunk::Data(delegate_bytes));
    }

    if let Some(mp) = opts.metaprotocol {
        chunks.push(ScriptChunk::Data(vec![TAG_METAPROTOCOL]));
        chunks.push(ScriptChunk::Data(mp.as_bytes().to_vec()));
    }

    if let Some(title) = opts.title {
        let props = encode_properties_title(title);
        for chunk in props.chunks(MAX_PUSH_SIZE) {
            chunks.push(ScriptChunk::Data(vec![TAG_PROPERTIES]));
            chunks.push(ScriptChunk::Data(chunk.to_vec()));
        }
    }

    if let Some(md) = opts.metadata {
        for chunk in md.chunks(MAX_PUSH_SIZE) {
            chunks.push(ScriptChunk::Data(vec![TAG_METADATA]));
            chunks.push(ScriptChunk::Data(chunk.to_vec()));
        }
    }

    if let Some(enc) = opts.content_encoding {
        chunks.push(ScriptChunk::Data(vec![TAG_CONTENT_ENCODING]));
        chunks.push(ScriptChunk::Data(enc.to_vec()));
    }

    if !opts.omit_content && !body.is_empty() {
        chunks.push(ScriptChunk::Data(vec![TAG_BODY]));
        for chunk in body.chunks(MAX_PUSH_SIZE) {
            chunks.push(ScriptChunk::Data(chunk.to_vec()));
        }
    }

    chunks.push(ScriptChunk::Op(OP_ENDIF));
    Ok(compile_script(&chunks))
}

/// Encode Properties CBOR: `{ 1: Attributes { 0: title } }` per
/// https://docs.ordinals.com/inscriptions/properties.html
pub fn encode_properties_title(title: &str) -> Vec<u8> {
    let t = title.as_bytes();
    let mut out = Vec::with_capacity(8 + t.len());
    out.push(0xa1); // map(1) Properties
    out.push(0x01); // key 1 = Attributes
    out.push(0xa1); // map(1) Attributes
    out.push(0x00); // key 0 = title
    push_cbor_text(&mut out, t);
    out
}

fn push_cbor_text(out: &mut Vec<u8>, t: &[u8]) {
    if t.len() < 24 {
        out.push(0x60 + t.len() as u8);
    } else if t.len() < 256 {
        out.push(0x78);
        out.push(t.len() as u8);
    } else {
        out.push(0x79);
        out.push((t.len() >> 8) as u8);
        out.push((t.len() & 0xff) as u8);
    }
    out.extend_from_slice(t);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_over_520_uses_two_pushes() {
        let body = vec![0x41u8; 521];
        let script = build_text_envelope(&body);
        assert_eq!(*script.last().unwrap(), OP_ENDIF);
        assert!(script.len() > 520);
        let marker = [0x4d, 0x08, 0x02];
        assert!(
            script.windows(3).any(|w| w == marker),
            "expected PUSHDATA2 for 520-byte chunk"
        );
    }

    #[test]
    fn parent_tag_present() {
        let id = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi0";
        let script = build_text_envelope_with_parent(b"child", Some(id)).unwrap();
        assert!(script.windows(3).any(|w| w == b"ord"));
        // Parent tag encoded as OP_3 (0x53) via minimal push of [3]
        assert!(script.contains(&0x53));
        assert!(script.windows(32).any(|w| w[0] == 0x1f && w[31] == 0x00));
    }

    #[test]
    fn delegate_tag_present_without_body() {
        let id = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi0";
        let script = build_delegate_envelope(id, None).unwrap();
        assert!(script.windows(3).any(|w| w == b"ord"));
        // Tag 11 → OP_PUSHNUM_11 = 0x5b
        assert!(script.contains(&0x5b), "expected OP_11 for delegate tag");
        assert!(!script.windows(4).any(|w| w == [0x01, 0x00, 0x01, 0x01])); // no body tag 0 push pair loosely
        // No content-type push of "text"
        assert!(!contains_slice(&script, b"text/plain"));
    }

    #[test]
    fn delegate_with_body_keeps_content_type() {
        let id = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi0";
        let script = build_delegate_envelope(id, Some(b"fallback")).unwrap();
        assert!(contains_slice(&script, b"text/plain"));
        assert!(script.contains(&0x5b));
    }

    #[test]
    fn title_goes_to_properties_tag_17_not_metadata() {
        let script = build_envelope(
            b"text/html",
            b"<html/>",
            EnvelopeOptions {
                title: Some("BHANG"),
                metaprotocol: Some("bhang"),
                ..Default::default()
            },
        )
        .unwrap();
        // Tag 17 is outside OP_1..OP_16 → PUSHBYTES_1 0x11
        assert!(script.windows(2).any(|w| w == [0x01, 0x11]), "tag 17 properties");
        assert!(contains_slice(&script, b"BHANG"));
        assert!(script.contains(&0x57)); // metaprotocol tag 7 → OP_7
        assert!(contains_slice(&script, b"bhang"));
    }

    #[test]
    fn properties_title_cbor_shape() {
        let cbor = encode_properties_title("BHANG");
        assert_eq!(cbor[0], 0xa1);
        assert_eq!(cbor[1], 0x01);
        assert_eq!(cbor[2], 0xa1);
        assert_eq!(cbor[3], 0x00);
        assert!(cbor.windows(5).any(|w| w == b"BHANG"));
    }

    #[test]
    fn metaprotocol_and_encoding_tags() {
        let script = build_envelope(
            b"text/plain",
            b"x",
            EnvelopeOptions {
                metaprotocol: Some("foo"),
                content_encoding: Some(b"br"),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(script.contains(&0x57)); // OP_7 metaprotocol
        assert!(script.contains(&0x59)); // OP_9 content_encoding
        assert!(contains_slice(&script, b"foo"));
        assert!(contains_slice(&script, b"br"));
    }

    fn contains_slice(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }
}
