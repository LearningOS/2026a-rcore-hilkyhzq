extern crate alloc;
#[path = "../os/src/mm/elf_validate.rs"]
mod elf_validate;
fn image() -> Vec<u8> {
    let mut v=vec![0u8;120]; v[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    v[16..18].copy_from_slice(&2u16.to_le_bytes()); v[18..20].copy_from_slice(&243u16.to_le_bytes());
    v[20..24].copy_from_slice(&1u32.to_le_bytes()); v[24..32].copy_from_slice(&4096u64.to_le_bytes());
    v[32..40].copy_from_slice(&64u64.to_le_bytes()); v[52..54].copy_from_slice(&64u16.to_le_bytes());
    v[54..56].copy_from_slice(&56u16.to_le_bytes()); v[56..58].copy_from_slice(&1u16.to_le_bytes());
    v[64..68].copy_from_slice(&1u32.to_le_bytes()); v[68..72].copy_from_slice(&5u32.to_le_bytes());
    v[80..88].copy_from_slice(&4096u64.to_le_bytes()); v[104..112].copy_from_slice(&4096u64.to_le_bytes());
    v
}
#[test] fn accepts_supported_minimal_image() { assert!(elf_validate::elf_can_load(&image())); }
#[test] fn rejects_plain_empty_and_truncated() {
    for v in [vec![],b"plain text".to_vec(),image()[..80].to_vec()] { assert!(!elf_validate::elf_can_load(&v)); }
}
#[test] fn rejects_bad_magic_class_and_machine() {
    for offset in [0,4,5,18] { let mut v=image(); v[offset]=0; assert!(!elf_validate::elf_can_load(&v)); }
}
#[test] fn rejects_overflowing_header_offset() {
    let mut v=image(); v[32..40].copy_from_slice(&u64::MAX.to_le_bytes()); assert!(!elf_validate::elf_can_load(&v));
}
#[test] fn rejects_file_range_outside_image() {
    let mut v=image(); v[96..104].copy_from_slice(&1000u64.to_le_bytes()); assert!(!elf_validate::elf_can_load(&v));
}
#[test] fn rejects_invalid_memory_extent_and_entry() {
    let mut v=image(); v[104..112].copy_from_slice(&u64::MAX.to_le_bytes()); assert!(!elf_validate::elf_can_load(&v));
    let mut v=image(); v[24..32].copy_from_slice(&0x8000u64.to_le_bytes()); assert!(!elf_validate::elf_can_load(&v));
}
