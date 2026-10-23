/// Fixtures BUS anteriores a v7 não tinham a seção PFIFO no final.
/// Só recebe snapshots de Bus produzidos nestes testes, sem payload arbitrário.
pub(super) fn without_pfifo(bytes: &[u8]) -> &[u8] {
    let at = bytes.windows(4).rposition(|v| v == b"PFFO").expect("PFFO");
    let count = u32::from_le_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
    let mut end = at + 16 + count * 4;
    let profile = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap());
    if profile == 2 {
        let len = u64::from_le_bytes(bytes[end..end + 8].try_into().unwrap()) as usize;
        end += 8 + len;
    }
    assert_eq!(end, bytes.len());
    &bytes[..at]
}
