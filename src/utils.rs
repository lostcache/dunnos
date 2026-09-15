pub(crate) fn read_be_u32_from_addr(addr: usize) -> u32 {
    let bytes = unsafe { core::slice::from_raw_parts(addr as *const u8, 4) };
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub(crate) fn read_be_u64_from_addr(addr: usize) -> u64 {
    ((read_be_u32_from_addr(addr) as u64) << 32) | read_be_u32_from_addr(addr + 4) as u64
}

pub(crate) fn align4(x: usize) -> usize {
    (x + 3) & !3
}

pub(crate) fn align_up(x: usize, align: usize) -> usize {
    (x + align - 1) & !(align - 1)
}

pub(crate) fn get_null_terminated_u8_slice<'a>(addr: usize, size: usize) -> Option<&'a [u8]> {
    let slice = unsafe { core::slice::from_raw_parts(addr as *const u8, size) };
    let len = slice.iter().position(|&b| b == 0)?;
    Some(&slice[..len])
}

pub(crate) const fn mb(x: usize) -> usize {
    x * 1024 * 1024
}
