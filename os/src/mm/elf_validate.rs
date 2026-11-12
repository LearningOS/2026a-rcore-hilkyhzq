//! Validate the static ELF64/RISC-V subset understood by the teaching loader.

fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
}
fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
fn u64_at(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}

/// Reject malformed headers, out-of-file segments and layouts the loader cannot map.
/// Allocation failure is still governed by the teaching kernel's memory assumption.
pub fn elf_can_load(data: &[u8]) -> bool {
    if data.len() < 64 || &data[..7] != b"\x7fELF\x02\x01\x01"
        || u16_at(data, 16) != 2 || u16_at(data, 18) != 243
        || u32_at(data, 20) != 1 || u16_at(data, 52) != 64
        || u16_at(data, 54) != 56 { return false; }
    let entry = u64_at(data, 24);
    let Ok(table) = usize::try_from(u64_at(data, 32)) else { return false; };
    let count = u16_at(data, 56) as usize;
    let Some(table_end) = count.checked_mul(56).and_then(|n| table.checked_add(n)) else { return false; };
    if count == 0 || table % 8 != 0 || table_end > data.len() { return false; }
    let mut previous_end = 0u64;
    let mut executable_entry = false;
    let mut loaded = false;
    for index in 0..count {
        let at = table + index * 56;
        let kind = u32_at(data, at);
        if kind > 7 && !(0x6000_0000..=0x7fff_ffff).contains(&kind) { return false; }
        if kind != 1 { continue; }
        let flags = u32_at(data, at + 4);
        let offset = u64_at(data, at + 8);
        let va = u64_at(data, at + 16);
        let file_size = u64_at(data, at + 32);
        let mem_size = u64_at(data, at + 40);
        let align = u64_at(data, at + 48);
        let Some(file_end) = offset.checked_add(file_size) else { return false; };
        let Some(end) = va.checked_add(mem_size) else { return false; };
        // The existing copy routine requires page-aligned virtual segment starts.
        if mem_size == 0 || file_size > mem_size || file_end > data.len() as u64
            || va % 4096 != 0 || va < previous_end || end > (1u64 << 38)
            || flags & !7 != 0 || flags & 5 == 0 || flags & 2 != 0 && flags & 4 == 0
            || align > 1 && (!align.is_power_of_two() || va % align != offset % align) {
            return false;
        }
        let Some(rounded) = end.checked_add(4095).map(|v| v & !4095) else { return false; };
        previous_end = rounded;
        loaded = true;
        executable_entry |= flags & 1 != 0 && va <= entry && entry < end;
    }
    // Reserve room for the initial stack and its guard pages in the lower half.
    loaded && executable_entry && previous_end <= (1u64 << 38) - 4 * 4096
}
