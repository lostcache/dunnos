use crate::utils::{self, read_be_u64_from_addr};
use core::{cell::SyncUnsafeCell, cmp::max};

const MAGIC_VALUE: u32 = 0xd00d_feed;
const FDT_BEGIN_NODE: u32 = 1;
const FDT_NODE_END: u32 = 2;
const FDT_PROP: u32 = 3;
const FDT_NOP: u32 = 4;
const FDT_END: u32 = 9;

const MAX_CLOSED_DEPTH: u32 = 64;

pub(crate) enum FdtError {
    NoDtb,
    BadHeader,
    BadStructure,
    ArenaFull,
}

struct FdtBlocks {
    struct_base: usize,
    size_dt_struct: usize,
    string_base: usize,
    size_dt_strings: usize,
    rsvmap_base: usize,
    end: usize,
}

pub(crate) fn probe() -> Result<(), FdtError> {
    let fdt_base = unsafe { crate::dtb_ptr };
    let blocks = parse_header(fdt_base)?;
    let count = count(&blocks)?;
    init_arena(&count)?;
    build(&blocks)?;
    Ok(())
}

fn parse_header(fdt_base: usize) -> Result<FdtBlocks, FdtError> {
    if fdt_base == 0 {
        return Err(FdtError::NoDtb);
    }

    if utils::read_be_u32_from_addr(fdt_base) != MAGIC_VALUE {
        return Err(FdtError::BadHeader);
    }

    let total_size = utils::read_be_u32_from_addr(fdt_base + 4) as usize;
    if total_size < 32 {
        return Err(FdtError::BadHeader);
    }

    let off_dt_struct = utils::read_be_u32_from_addr(fdt_base + 8) as usize;
    let off_dt_strings = utils::read_be_u32_from_addr(fdt_base + 12) as usize;
    let off_mem_rsvmap = utils::read_be_u32_from_addr(fdt_base + 16) as usize;
    let version = utils::read_be_u32_from_addr(fdt_base + 20) as usize;
    let last_comp_version = utils::read_be_u32_from_addr(fdt_base + 24) as usize;

    if version < 16 || last_comp_version > 17 {
        return Err(FdtError::BadHeader);
    }

    let (size_dt_struct, size_dt_strings) = if version >= 17 {
        if total_size < 40 {
            return Err(FdtError::BadHeader);
        }
        (
            utils::read_be_u32_from_addr(fdt_base + 36) as usize,
            utils::read_be_u32_from_addr(fdt_base + 32) as usize,
        )
    } else {
        if off_dt_strings < off_dt_struct {
            return Err(FdtError::BadHeader);
        }
        (off_dt_strings - off_dt_struct, total_size - off_dt_strings)
    };

    if !off_dt_struct.is_multiple_of(4) || !off_mem_rsvmap.is_multiple_of(8) {
        return Err(FdtError::BadHeader);
    }

    if off_dt_struct + size_dt_struct > total_size
        || off_dt_strings + size_dt_strings > total_size
        || off_mem_rsvmap + 16 > total_size
    {
        return Err(FdtError::BadHeader);
    }

    Ok(FdtBlocks {
        struct_base: fdt_base + off_dt_struct,
        size_dt_struct,
        string_base: fdt_base + off_dt_strings,
        size_dt_strings,
        rsvmap_base: fdt_base + off_mem_rsvmap,
        end: fdt_base + total_size,
    })
}

struct Counts {
    nodes: u32,
    props: u32,
    max_depth: u32,
    reserved: u32,
}

fn count(b: &FdtBlocks) -> Result<Counts, FdtError> {
    let mut cursor = b.struct_base;
    let struct_end = b.struct_base + b.size_dt_struct;
    let strings_end = b.string_base + b.size_dt_strings;
    let mut closed = 0u64;
    let mut depth: i32 = -1;
    let mut node_count = 0u32;
    let mut prop_count = 0u32;
    let mut max_depth = 0u32;
    let mut seen_end = false;

    while cursor + 4 <= struct_end {
        let token = utils::read_be_u32_from_addr(cursor);
        cursor += 4;
        match token {
            FDT_BEGIN_NODE => {
                depth += 1;
                if depth >= MAX_CLOSED_DEPTH as i32 {
                    return Err(FdtError::BadStructure);
                }

                let Some(name) = utils::get_null_terminated_u8_slice(cursor, struct_end - cursor)
                else {
                    return Err(FdtError::BadStructure);
                };
                cursor = utils::align4(cursor + name.len() + 1); // why + 1?
                if cursor > struct_end {
                    return Err(FdtError::BadStructure);
                }

                if depth > 0 {
                    closed |= 1 << (depth - 1);
                }
                node_count = node_count.checked_add(1).ok_or(FdtError::BadStructure)?;
                max_depth = max(max_depth, (depth + 1) as u32);
            }
            FDT_PROP => {
                if depth < 0 || closed & (1 << depth) != 0 {
                    return Err(FdtError::BadStructure);
                }

                let prop_len = utils::read_be_u32_from_addr(cursor) as usize;
                let name_off = utils::read_be_u32_from_addr(cursor + 4) as usize;
                cursor += 8;
                if cursor + prop_len >= struct_end || b.string_base + name_off >= strings_end {
                    return Err(FdtError::BadStructure);
                }

                if utils::get_null_terminated_u8_slice(
                    b.string_base + name_off,
                    b.size_dt_strings - name_off,
                )
                .is_none()
                {
                    return Err(FdtError::BadStructure);
                }
                cursor = utils::align4(cursor + prop_len);
                if cursor >= struct_end {
                    return Err(FdtError::BadStructure);
                }
                prop_count = prop_count.checked_add(1).ok_or(FdtError::BadStructure)?;
            }
            FDT_NODE_END => {
                if depth < 0 {
                    return Err(FdtError::BadStructure);
                }
                closed &= !(1 << depth);
                depth -= 1;
            }
            FDT_NOP => {}
            FDT_END => {
                seen_end = true;
                break;
            }
            _ => return Err(FdtError::BadStructure),
        }
    }

    if !seen_end || depth != -1 {
        return Err(FdtError::BadStructure);
    }

    cursor = b.rsvmap_base;
    let mut reserved = 0u32;
    loop {
        if cursor + 16 >= b.end {
            return Err(FdtError::BadStructure);
        }

        if utils::read_be_u64_from_addr(cursor) == 0
            && utils::read_be_u64_from_addr(cursor + 8) == 0
        {
            break;
        }

        reserved = reserved.checked_add(1).ok_or(FdtError::BadStructure)?;
        cursor += 16;
    }

    Ok(Counts {
        nodes: node_count,
        props: prop_count,
        max_depth,
        reserved,
    })
}

fn build(b: &FdtBlocks) -> Result<(), FdtError> {
    let arena = get_mut_arena();

    let mut cursor = b.struct_base;
    let struct_end = b.struct_base + b.size_dt_struct;
    let mut node_idx = 0usize;
    let mut prop_idx = 0usize;
    let mut sp = 0usize;

    while cursor + 4 <= struct_end {
        let token = utils::read_be_u32_from_addr(cursor);
        cursor += 4;
        match token {
            FDT_BEGIN_NODE => {
                let Some(name) = utils::get_null_terminated_u8_slice(cursor, struct_end - cursor)
                else {
                    return Err(FdtError::BadStructure);
                };
                cursor = utils::align4(cursor + name.len() + 1);
                arena.nodes[node_idx] = Node {
                    name,
                    paren_idx: None,
                    first_child_idx: None,
                    next_sibling_idx: None,
                    first_prop_idx: None,
                    prop_count: 0,
                };
                if sp == 0 {
                    arena.root = node_idx;
                } else {
                    let paren_frame = &mut arena.frames[sp - 1];
                    let paren = paren_frame.node_idx;
                    arena.nodes[node_idx].paren_idx = Some(paren);
                    if let Some(paren_last_child) = paren_frame.last_child_idx {
                        arena.nodes[paren_last_child].next_sibling_idx = Some(node_idx);
                    } else {
                        arena.nodes[paren].first_child_idx = Some(node_idx);
                    }
                    paren_frame.last_child_idx = Some(node_idx);
                }
                arena.frames[sp] = Frame {
                    node_idx,
                    last_child_idx: None,
                };
                sp += 1;
                node_idx += 1;
            }
            FDT_PROP => {
                let len = utils::read_be_u32_from_addr(cursor) as usize;
                let name_off = utils::read_be_u32_from_addr(cursor + 4) as usize;
                cursor += 8;
                let Some(name) = utils::get_null_terminated_u8_slice(
                    b.string_base + name_off,
                    b.size_dt_strings - name_off,
                ) else {
                    return Err(FdtError::BadStructure);
                };
                let prop_str_slice =
                    unsafe { core::slice::from_raw_parts(cursor as *const u8, len) };
                cursor = utils::align4(cursor + len);
                let node_idx = arena.frames[sp - 1].node_idx;
                if arena.nodes[node_idx].prop_count == 0 {
                    arena.nodes[node_idx].first_prop_idx = Some(prop_idx);
                }
                arena.nodes[node_idx].prop_count += 1;
                arena.props[prop_idx] = Property {
                    name,
                    prop_str_slice,
                };
                prop_idx += 1;
            }
            FDT_NODE_END => sp -= 1,
            FDT_NOP => {}
            FDT_END => break,
            _ => return Err(FdtError::BadStructure),
        }
    }

    cursor = b.rsvmap_base;
    for r in arena.reserved.iter_mut() {
        *r = Region {
            base: read_be_u64_from_addr(cursor),
            size: read_be_u64_from_addr(cursor + 8),
        };
        cursor += 16;
    }

    Ok(())
}

struct Node {
    name: &'static [u8],
    paren_idx: Option<usize>,
    first_child_idx: Option<usize>,
    next_sibling_idx: Option<usize>,
    first_prop_idx: Option<usize>,
    prop_count: u32,
}

struct Property {
    name: &'static [u8],
    prop_str_slice: &'static [u8],
}

struct Frame {
    node_idx: usize,
    last_child_idx: Option<usize>,
}

pub(crate) struct Region {
    pub(crate) base: u64,
    size: u64,
}

const ARENA_SIZE_BYTES: usize = 64 * 1024;
struct Arena {
    data: [u8; ARENA_SIZE_BYTES],
    used: usize,
    node_count: u32,
    prop_count: u32,
    reserved_count: u32,
    root: usize,
    nodes: &'static mut [Node],
    props: &'static mut [Property],
    reserved: &'static mut [Region],
    frames: &'static mut [Frame],
}

static ARENA: SyncUnsafeCell<Arena> = SyncUnsafeCell::new(Arena {
    data: [0; ARENA_SIZE_BYTES],
    used: 0,
    node_count: 0,
    prop_count: 0,
    reserved_count: 0,
    root: 0,
    nodes: &mut [],
    props: &mut [],
    reserved: &mut [],
    frames: &mut [],
});

fn get_mut_arena() -> &'static mut Arena {
    unsafe { &mut *ARENA.get() }
}

fn get_arena() -> &'static Arena {
    unsafe { &*ARENA.get() }
}

fn init_arena(c: &Counts) -> Result<(), FdtError> {
    let arena = get_mut_arena();
    let arena_base = arena.data.as_mut_ptr();

    let nodes_off = arena_alloc(c.nodes as usize * size_of::<Node>(), align_of::<Node>())?;
    arena.nodes = unsafe {
        core::slice::from_raw_parts_mut(arena_base.add(nodes_off).cast::<Node>(), c.nodes as usize)
    };

    let props_off = arena_alloc(
        c.props as usize * size_of::<Property>(),
        align_of::<Property>(),
    )?;
    arena.props = unsafe {
        core::slice::from_raw_parts_mut(
            arena_base.add(props_off).cast::<Property>(),
            c.props as usize,
        )
    };

    let frames_off = arena_alloc(
        c.max_depth as usize * size_of::<Frame>(),
        align_of::<Frame>(),
    )?;
    arena.frames = unsafe {
        core::slice::from_raw_parts_mut(
            arena_base.add(frames_off).cast::<Frame>(),
            c.max_depth as usize,
        )
    };

    let reserved_off = arena_alloc(
        c.reserved as usize * size_of::<Region>(),
        align_of::<Region>(),
    )?;
    arena.reserved = unsafe {
        core::slice::from_raw_parts_mut(
            arena_base.add(reserved_off).cast::<Region>(),
            c.reserved as usize,
        )
    };

    Ok(())
}

fn arena_alloc(size: usize, align: usize) -> Result<usize, FdtError> {
    let a = get_mut_arena();
    let start = utils::align_up(a.used, align);
    let end = start.checked_add(size).ok_or(FdtError::ArenaFull)?;
    if end > ARENA_SIZE_BYTES {
        return Err(FdtError::ArenaFull);
    }
    a.used = end;
    Ok(start)
}

pub(crate) fn find_compatible(s: &[u8]) -> Option<usize> {
    (0..get_mut_arena().nodes.len()).find(|&id| compatible_has(id, s))
}

pub(crate) fn compatible_has(id: usize, s: &[u8]) -> bool {
    let Some(v) = prop(id, b"compatible") else {
        return false;
    };
    v.split(|&b| b == 0).any(|x| x == s)
}

pub(crate) fn reg(id: usize, i: usize) -> Option<Region> {
    let v = prop(id, b"reg")?;
    let ac = address_cells(id);
    let sc = size_cells(id);
    let width = (ac + sc) * 4;
    if width == 0 {
        return None;
    }
    let off = i.checked_mul(width)?;
    let end = off.checked_add(width)?;
    if end > v.len() {
        return None;
    }
    let base = decode_cells(&v[off..off + ac * 4])?;
    let size = if sc == 0 {
        0
    } else {
        decode_cells(&v[off + ac * 4..end])?
    };
    Some(Region { base, size })
}

fn decode_cells(cells: &[u8]) -> Option<u64> {
    match cells.len() {
        4 => Some(u32::from_be_bytes([cells[0], cells[1], cells[2], cells[3]]) as u64),
        8 => {
            let hi = u32::from_be_bytes([cells[0], cells[1], cells[2], cells[3]]) as u64;
            let lo = u32::from_be_bytes([cells[4], cells[5], cells[6], cells[7]]) as u64;
            Some((hi << 32) | lo)
        }
        _ => None,
    }
}

pub(crate) fn size_cells(id: usize) -> usize {
    parent_cells(id, b"#size-cells", 1)
}

pub(crate) fn address_cells(id: usize) -> usize {
    parent_cells(id, b"#address-cells", 2)
}

fn parent_cells(id: usize, name: &[u8], default: usize) -> usize {
    let mut cur = node(id).and_then(|n| n.paren_idx);
    while let Some(p) = cur {
        if let Some(v) = prop_u32(p, name) {
            return v as usize;
        }
        cur = node(p).and_then(|n| n.paren_idx);
    }
    default
}

pub(crate) fn prop_u32(id: usize, name: &[u8]) -> Option<u32> {
    let v = prop(id, name)?;
    if v.len() != 4 {
        return None;
    }
    Some(u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
}

pub(crate) fn prop(id: usize, name: &[u8]) -> Option<&'static [u8]> {
    props(id)
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.prop_str_slice)
}

pub(crate) fn props(id: usize) -> &'static [Property] {
    let Some(n) = nodes().get(id) else {
        return &[];
    };
    let Some(start) = n.first_prop_idx else {
        return &[];
    };
    let end = start + n.prop_count as usize;
    get_arena().props.get(start..end).unwrap_or(&[])
}

pub(crate) fn node(id: usize) -> Option<&'static Node> {
    nodes().get(id)
}

fn nodes() -> &'static [Node] {
    get_arena().nodes
}
