//! Ordinals inscription envelope helpers.

pub mod compress;
pub mod envelope;
pub mod parent_id;
pub mod script_push;
pub mod tapscript;

pub const PROTOCOL_TAG: &[u8] = b"ord";

pub use compress::{brotli_compress, brotli_recommended_for};
pub use envelope::{
    build_delegate_envelope, build_envelope, build_text_envelope, build_text_envelope_with_parent,
    encode_properties_title, EnvelopeOptions,
};
pub use parent_id::{encode_inscription_id, InscriptionIdError};
pub use tapscript::{
    build_delegate_tapscript, build_inscription_tapscript, build_inscription_tapscript_opts,
    build_inscription_tapscript_with_parent,
};
