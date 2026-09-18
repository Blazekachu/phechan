//! Minimal Runestone-ish helpers for **co-spend awareness** — not an etch product.

/// Flag bytes / markers we may encounter when inspecting unknown OP_RETURNs.
/// Full Runestone encode/decode lives outside Phechan's inscription focus.
pub const RUNESTONE_MAGIC: &[u8] = b"RUNE_TEST"; // placeholder marker for tests only

/// Encode a **test-only** OP_RETURN payload that signals "runes co-spend expected".
/// Production runestones must use the real Runes encoding (see `ord` / runes spec).
pub fn encode_cospend_marker(note: &str) -> Vec<u8> {
    let mut out = RUNESTONE_MAGIC.to_vec();
    out.extend_from_slice(note.as_bytes());
    out
}

pub fn looks_like_test_cospend_marker(data: &[u8]) -> bool {
    data.starts_with(RUNESTONE_MAGIC)
}

/// Advice string for UI/CLI when a UTXO is rune-bearing.
pub fn cospend_advice(has_validated_runestone: bool) -> &'static str {
    if has_validated_runestone {
        "rune-bearing input with validated runestone — still verify balances"
    } else {
        "rune-bearing input without validated runestone — refuse silent burn by default"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_round_trip() {
        let b = encode_cospend_marker("demo");
        assert!(looks_like_test_cospend_marker(&b));
        assert!(!looks_like_test_cospend_marker(b"nope"));
    }
}
