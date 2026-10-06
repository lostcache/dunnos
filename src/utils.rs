pub(crate) fn read_be_u32_from_addr(addr: usize) -> u32 {
    u32::from_be(unsafe { core::ptr::read_unaligned(addr as *const u32) })
}

pub(crate) fn read_be_u64_from_addr(addr: usize) -> u64 {
    u64::from_be(unsafe { core::ptr::read_unaligned(addr as *const u64) })
}

pub(crate) fn get_null_terminated_u8_slice<'a>(addr: usize, size: usize) -> Option<&'a [u8]> {
    let slice = unsafe { core::slice::from_raw_parts(addr as *const u8, size) };
    let len = slice.iter().position(|&b| b == 0)?;
    Some(&slice[..len])
}

pub(crate) fn read_be_u32_from_bytes(bytes: &[u8]) -> Option<u32> {
    let &[a, b, c, d, ..] = bytes else {
        return None;
    };
    Some(u32::from_be_bytes([a, b, c, d]))
}

pub(crate) fn current_hart_id() -> Option<u32> {
    let id: usize;
    unsafe {
        core::arch::asm!("csrr {}, mhartid", out(reg) id);
    }
    u32::try_from(id).ok()
}

pub const fn mb(x: usize) -> usize {
    x * 1024 * 1024
}
