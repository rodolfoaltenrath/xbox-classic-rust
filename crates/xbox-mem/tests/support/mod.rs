/// Fixtures BUS anteriores a v7 não tinham a seção PFIFO no final.
/// Só recebe snapshots de Bus produzidos nestes testes, sem payload arbitrário.
pub(super) fn without_pfifo(bytes: &[u8]) -> &[u8] {
    let at = bytes.windows(4).rposition(|v| v == b"PFFO").expect("PFFO");
    let count = u32::from_le_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
    assert_eq!(at + 16 + count * 4, bytes.len());
    &bytes[..at]
}
