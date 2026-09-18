//! BIP62.3-oriented Bitcoin script push compiler (minimal pushes).

pub const OP_FALSE: u8 = 0x00;
pub const OP_IF: u8 = 0x63;
pub const OP_ENDIF: u8 = 0x68;
pub const OP_CHECKSIG: u8 = 0xac;
pub const OP_PUSHDATA1: u8 = 0x4c;
pub const OP_PUSHDATA2: u8 = 0x4d;
pub const OP_PUSHDATA4: u8 = 0x4e;
pub const MAX_PUSH_SIZE: usize = 520;

#[derive(Debug, Clone)]
pub enum ScriptChunk {
    Op(u8),
    Data(Vec<u8>),
}

fn push_data_prefix(len: usize) -> Vec<u8> {
    if len < OP_PUSHDATA1 as usize {
        vec![len as u8]
    } else if len <= 0xff {
        vec![OP_PUSHDATA1, len as u8]
    } else if len <= 0xffff {
        vec![OP_PUSHDATA2, (len & 0xff) as u8, ((len >> 8) & 0xff) as u8]
    } else {
        vec![
            OP_PUSHDATA4,
            (len & 0xff) as u8,
            ((len >> 8) & 0xff) as u8,
            ((len >> 16) & 0xff) as u8,
            ((len >> 24) & 0xff) as u8,
        ]
    }
}

/// Compile opcodes and data pushes into a script byte vector.
pub fn compile_script(chunks: &[ScriptChunk]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in chunks {
        match chunk {
            ScriptChunk::Op(op) => out.push(*op),
            ScriptChunk::Data(data) => {
                if data.is_empty() {
                    out.push(0x00); // OP_0
                    continue;
                }
                if data.len() == 1 {
                    let b = data[0];
                    if b == 0x00 {
                        out.push(0x00);
                        continue;
                    }
                    if (0x01..=0x10).contains(&b) {
                        out.push(0x50 + b);
                        continue;
                    }
                    if b == 0x81 {
                        out.push(0x4f); // OP_1NEGATE
                        continue;
                    }
                }
                out.extend_from_slice(&push_data_prefix(data.len()));
                out.extend_from_slice(data);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_push_op1() {
        let script = compile_script(&[ScriptChunk::Data(vec![0x01])]);
        assert_eq!(script, vec![0x51]); // OP_1
    }
}
