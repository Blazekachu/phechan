//! Encode inscription IDs for parent/delegate tags (ord binary format).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InscriptionIdError {
    InvalidFormat,
    BadTxid,
    BadIndex,
}

impl std::fmt::Display for InscriptionIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "inscription id must look like <64hex>i<index>"),
            Self::BadTxid => write!(f, "invalid txid hex"),
            Self::BadIndex => write!(f, "invalid inscription index"),
        }
    }
}

impl std::error::Error for InscriptionIdError {}

/// Encode `txid_hex i index` as little-endian txid bytes + LE index with trailing zeros omitted.
pub fn encode_inscription_id(inscription_id: &str) -> Result<Vec<u8>, InscriptionIdError> {
    let (txid_hex, index_str) = inscription_id
        .rsplit_once('i')
        .ok_or(InscriptionIdError::InvalidFormat)?;
    if txid_hex.len() != 64 {
        return Err(InscriptionIdError::InvalidFormat);
    }
    let index: u32 = index_str
        .parse()
        .map_err(|_| InscriptionIdError::BadIndex)?;
    let mut txid = hex_to_bytes(txid_hex).ok_or(InscriptionIdError::BadTxid)?;
    txid.reverse(); // display hex is reversed vs internal byte order
    if index == 0 {
        return Ok(txid);
    }
    let mut index_bytes = Vec::new();
    let mut idx = index;
    while idx > 0 {
        index_bytes.push((idx & 0xff) as u8);
        idx >>= 8;
    }
    txid.extend_from_slice(&index_bytes);
    Ok(txid)
}

fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_i0_is_32_bytes_reversed() {
        let id = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi0";
        let bytes = encode_inscription_id(id).unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 0x1f);
        assert_eq!(bytes[31], 0x00);
    }
}
