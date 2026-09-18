use phechan_ordinals::envelope::build_text_envelope;

#[test]
fn hello_world_envelope_matches_handbook_shape() {
    let script = build_text_envelope(b"Hello, world!");
    let ord = b"ord";
    assert!(script.windows(ord.len()).any(|w| w == ord));
    let ctype = b"text/plain;charset=utf-8";
    assert!(script.windows(ctype.len()).any(|w| w == ctype));
    assert!(script.windows(13).any(|w| w == b"Hello, world!"));
    assert_eq!(script[0], 0x00); // OP_FALSE
    assert_eq!(script[1], 0x63); // OP_IF
    assert_eq!(*script.last().unwrap(), 0x68); // OP_ENDIF
}
