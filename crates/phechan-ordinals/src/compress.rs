//! Optional body compression (content_encoding tag 9).

use std::io::Write;

/// MIME types where Brotli on the inscription body is commonly useful.
pub fn brotli_recommended_for(content_type: &str) -> bool {
    let ct = content_type.split(';').next().unwrap_or(content_type).trim().to_ascii_lowercase();
    ct.starts_with("text/")
        || ct == "application/json"
        || ct == "application/javascript"
        || ct == "application/xml"
        || ct == "image/svg+xml"
}

/// Compress `body` with Brotli. Returns compressed bytes.
pub fn brotli_compress(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut out, 4096, 5, 22);
        writer
            .write_all(body)
            .map_err(|e| format!("brotli write: {e}"))?;
        writer.flush().map_err(|e| format!("brotli flush: {e}"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_recommended() {
        assert!(brotli_recommended_for("text/html;charset=utf-8"));
        assert!(!brotli_recommended_for("image/png"));
    }

    #[test]
    fn compress_shrinks_repetitive() {
        let body = b"<html>".repeat(200);
        let c = brotli_compress(&body).unwrap();
        assert!(c.len() < body.len());
    }
}
