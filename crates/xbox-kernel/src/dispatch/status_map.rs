//! `RtlNtStatusToDosError` (ADR-0103, P31): tabela **provisória** dos status que este kernel
//! emite, com os códigos Win32 da documentação pública do NT. Status fora da tabela devolvem
//! `ERROR_MR_MID_NOT_FOUND` (317), o resultado documentado para status sem mapeamento.

const ERROR_MR_MID_NOT_FOUND: u32 = 317;

/// Pares (`NTSTATUS`, erro Win32).
const TABLE: &[(u32, u32)] = &[
    (0x0000_0000, 0),    // STATUS_SUCCESS -> ERROR_SUCCESS
    (0x0000_0102, 258),  // STATUS_TIMEOUT -> WAIT_TIMEOUT
    (0x0000_0103, 997),  // STATUS_PENDING -> ERROR_IO_PENDING
    (0xC000_0001, 31),   // STATUS_UNSUCCESSFUL -> ERROR_GEN_FAILURE
    (0xC000_0004, 24),   // STATUS_INFO_LENGTH_MISMATCH -> ERROR_BAD_LENGTH
    (0xC000_0005, 998),  // STATUS_ACCESS_VIOLATION -> ERROR_NOACCESS
    (0xC000_0008, 6),    // STATUS_INVALID_HANDLE -> ERROR_INVALID_HANDLE
    (0xC000_000D, 87),   // STATUS_INVALID_PARAMETER -> ERROR_INVALID_PARAMETER
    (0xC000_0011, 38),   // STATUS_END_OF_FILE -> ERROR_HANDLE_EOF
    (0xC000_0017, 8),    // STATUS_NO_MEMORY -> ERROR_NOT_ENOUGH_MEMORY
    (0xC000_0022, 5),    // STATUS_ACCESS_DENIED -> ERROR_ACCESS_DENIED
    (0xC000_0023, 122),  // STATUS_BUFFER_TOO_SMALL -> ERROR_INSUFFICIENT_BUFFER
    (0xC000_0034, 2),    // STATUS_OBJECT_NAME_NOT_FOUND -> ERROR_FILE_NOT_FOUND
    (0xC000_0035, 183),  // STATUS_OBJECT_NAME_COLLISION -> ERROR_ALREADY_EXISTS
    (0xC000_0043, 32),   // STATUS_SHARING_VIOLATION -> ERROR_SHARING_VIOLATION
    (0xC000_0046, 288),  // STATUS_MUTANT_NOT_OWNED -> ERROR_NOT_OWNER
    (0xC000_0047, 298),  // STATUS_SEMAPHORE_LIMIT_EXCEEDED -> ERROR_TOO_MANY_POSTS
    (0xC000_009A, 1450), // STATUS_INSUFFICIENT_RESOURCES -> ERROR_NO_SYSTEM_RESOURCES
];

/// Converte um `NTSTATUS` no erro Win32 correspondente.
pub fn nt_status_to_dos_error(status: u32) -> u32 {
    TABLE
        .iter()
        .find(|&&(nt, _)| nt == status)
        .map_or(ERROR_MR_MID_NOT_FOUND, |&(_, dos)| dos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_conhecidos_e_desconhecidos() {
        assert_eq!(nt_status_to_dos_error(0), 0);
        assert_eq!(nt_status_to_dos_error(0xC000_0034), 2);
        assert_eq!(nt_status_to_dos_error(0xC000_0022), 5);
        assert_eq!(nt_status_to_dos_error(0xC123_4567), 317);
    }

    #[test]
    fn tabela_sem_status_repetido() {
        for (i, (nt, _)) in TABLE.iter().enumerate() {
            assert!(
                TABLE[i + 1..].iter().all(|(other, _)| other != nt),
                "{nt:#x}"
            );
        }
    }
}
