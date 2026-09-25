use crate::utils::{self, read_be_u64_from_addr};
use core::{cell::SyncUnsafeCell, cmp::max};

static mut FDT_TOTAL_SIZE: u32 = 0;

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

/// Parses the DTB into the arena. Fails when the header or the
/// structure is invalid or the arena is too small.
pub(crate) fn probe() -> Result<(), FdtError> {
    let fdt_base = unsafe { crate::dtb_ptr };
    let blocks = parse_header(fdt_base)?;
    let counts = count(&blocks)?;
    init_arena(&counts)?;
    parse_dt(&blocks)?;
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

    unsafe { FDT_TOTAL_SIZE = total_size as u32 };

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

/// Pass 1: counts Nodes, Properties, maximum depth, and reserved
/// memory regions. This size the arena slices.
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
                if depth >= MAX_CLOSED_DEPTH.cast_signed() {
                    return Err(FdtError::BadStructure);
                }

                let Some(name) = utils::get_null_terminated_u8_slice(cursor, struct_end - cursor)
                else {
                    return Err(FdtError::BadStructure);
                };
                cursor = (cursor + name.len() + 1).next_multiple_of(4); // + 1 for the NUL terminator
                if cursor > struct_end {
                    return Err(FdtError::BadStructure);
                }

                if depth > 0 {
                    closed |= 1 << (depth - 1);
                }
                node_count = node_count.checked_add(1).ok_or(FdtError::BadStructure)?;
                max_depth = max(max_depth, (depth + 1).cast_unsigned());
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
                cursor = (cursor + prop_len).next_multiple_of(4);
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

/// Pass 2: fills the arena slices with node and property entries, and
/// links parents, children, and siblings.
fn parse_dt(b: &FdtBlocks) -> Result<(), FdtError> {
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
                cursor = (cursor + name.len() + 1).next_multiple_of(4);
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
                let value = unsafe { core::slice::from_raw_parts(cursor as *const u8, len) };
                cursor = (cursor + len).next_multiple_of(4);
                let cur_node_idx = arena.frames[sp - 1].node_idx;
                if arena.nodes[cur_node_idx].prop_count == 0 {
                    arena.nodes[cur_node_idx].first_prop_idx = Some(prop_idx);
                }
                arena.nodes[cur_node_idx].prop_count += 1;
                arena.props[prop_idx] = Property { name, value };
                prop_idx += 1;
            }
            FDT_NODE_END => sp -= 1,
            FDT_NOP => {}
            FDT_END => break,
            _ => return Err(FdtError::BadStructure),
        }
    }

    cursor = b.rsvmap_base;
    for region in arena.reserved.iter_mut() {
        *region = Resource {
            base: read_be_u64_from_addr(cursor),
            size: read_be_u64_from_addr(cursor + 8),
        };
        cursor += 16;
    }

    Ok(())
}

pub(crate) struct Node {
    name: &'static [u8],
    paren_idx: Option<usize>,
    first_child_idx: Option<usize>,
    next_sibling_idx: Option<usize>,
    first_prop_idx: Option<usize>,
    prop_count: u32,
}

pub(crate) struct Property {
    name: &'static [u8],
    value: &'static [u8],
}

struct Frame {
    node_idx: usize,
    last_child_idx: Option<usize>,
}

pub(crate) struct Resource {
    pub(crate) base: u64,
    pub(crate) size: u64,
}

const ARENA_SIZE_BYTES: usize = 64 * 1024;
struct Arena {
    data: [u8; ARENA_SIZE_BYTES],
    used: usize,
    root: usize,
    nodes: &'static mut [Node],
    props: &'static mut [Property],
    reserved: &'static mut [Resource],
    frames: &'static mut [Frame],
}

static ARENA: SyncUnsafeCell<Arena> = SyncUnsafeCell::new(Arena {
    data: [0; ARENA_SIZE_BYTES],
    used: 0,
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

/// Reserves the node, property, traversal-frame, and reserved-region
/// slices in the arena data block.
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
        c.reserved as usize * size_of::<Resource>(),
        align_of::<Resource>(),
    )?;
    arena.reserved = unsafe {
        core::slice::from_raw_parts_mut(
            arena_base.add(reserved_off).cast::<Resource>(),
            c.reserved as usize,
        )
    };

    Ok(())
}

/// Bump-allocates aligned bytes from the arena and returns the offset
/// from its start.
fn arena_alloc(size: usize, align: usize) -> Result<usize, FdtError> {
    let a = get_mut_arena();
    let start = a.used.next_multiple_of(align);
    let end = start.checked_add(size).ok_or(FdtError::ArenaFull)?;
    if end > ARENA_SIZE_BYTES {
        return Err(FdtError::ArenaFull);
    }
    a.used = end;
    Ok(start)
}

/// Returns the index of the first node whose compatible property lists
/// the given string.
pub(crate) fn find_compatible_node_idx(s: &[u8]) -> Option<usize> {
    (0..nodes().len()).find(|&node_idx| compatible_has(node_idx, s))
}

/// Returns true when the compatible property of a node lists the given
/// string.
pub(crate) fn compatible_has(node_idx: usize, s: &[u8]) -> bool {
    let Some(v) = get_node_prop_value_by_name(node_idx, b"compatible") else {
        return false;
    };
    v.split(|&b| b == 0).any(|x| x == s)
}

/// Returns the resource at the given position from a node's reg
/// property. The cell counts come from the nearest ancestor with
/// #address-cells and #size-cells.
pub(crate) fn get_resource(node_idx: usize, resource_idx: usize) -> Option<Resource> {
    let resource = get_node_prop_value_by_name(node_idx, b"reg")?;
    let ac = address_cells(node_idx);
    let sc = size_cells(node_idx);
    let width_bytes = (ac + sc) * 4;
    if width_bytes == 0 {
        return None;
    }
    let off = resource_idx.checked_mul(width_bytes)?;
    let end = off.checked_add(width_bytes)?;
    if end > resource.len() {
        return None;
    }
    let base = decode_cells(&resource[off..off + ac * 4])?;
    let size = if sc == 0 {
        0
    } else {
        decode_cells(&resource[off + ac * 4..end])?
    };
    Some(Resource { base, size })
}

fn decode_cells(cells: &[u8]) -> Option<u64> {
    match cells.len() {
        4 => Some(u64::from(u32::from_be_bytes([
            cells[0], cells[1], cells[2], cells[3],
        ]))),
        8 => {
            let hi = u64::from(u32::from_be_bytes([cells[0], cells[1], cells[2], cells[3]]));
            let lo = u64::from(u32::from_be_bytes([cells[4], cells[5], cells[6], cells[7]]));
            Some((hi << 32) | lo)
        }
        _ => None,
    }
}

/// Returns the #size-cells of the nearest ancestor, 1 when unset.
pub(crate) fn size_cells(node_idx: usize) -> usize {
    get_parent_prop_by_name(node_idx, b"#size-cells", 1) // DT spec default
}

/// Returns the #address-cells of the nearest ancestor, 2 when unset.
pub(crate) fn address_cells(node_idx: usize) -> usize {
    get_parent_prop_by_name(node_idx, b"#address-cells", 2) // DT spec default
}

fn get_parent_prop_by_name(node_idx: usize, name: &[u8], default: usize) -> usize {
    let mut parent = get_node_from_arena_by_idx(node_idx).and_then(|n| n.paren_idx);
    while let Some(p) = parent {
        if let Some(v) = find_node_u32_sized_prop_by_name(p, name) {
            return v as usize;
        }
        parent = get_node_from_arena_by_idx(p).and_then(|n| n.paren_idx);
    }
    default
}

/// Returns the u32 value of a named property of a node. None when the
/// property is absent or its length is not 4 bytes.
pub(crate) fn find_node_u32_sized_prop_by_name(node_idx: usize, name: &[u8]) -> Option<u32> {
    let v = get_node_prop_value_by_name(node_idx, name)?;
    if v.len() != 4 {
        return None;
    }
    Some(u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
}

/// Returns the raw bytes of a named property of a node.
pub(crate) fn get_node_prop_value_by_name(node_idx: usize, name: &[u8]) -> Option<&'static [u8]> {
    get_node_props(node_idx)
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.value)
}

/// Returns the property slice of a node. Empty when the node index is
/// out of range or the node has no property.
pub(crate) fn get_node_props(node_idx: usize) -> &'static [Property] {
    let Some(n) = nodes().get(node_idx) else {
        return &[];
    };
    let Some(start) = n.first_prop_idx else {
        return &[];
    };
    let end = start + n.prop_count as usize;
    get_arena().props.get(start..end).unwrap_or(&[])
}

/// Returns a node by index. None when the index is out of range.
pub(crate) fn get_node_from_arena_by_idx(node_idx: usize) -> Option<&'static Node> {
    nodes().get(node_idx)
}

fn nodes() -> &'static [Node] {
    get_arena().nodes
}

/// Returns the parent node index.
pub(crate) fn get_parent_by_node_idx(node_idx: usize) -> Option<usize> {
    get_node_from_arena_by_idx(node_idx)?.paren_idx
}

/// Returns the phandle of a node. Accepts the legacy `linux,phandle`.
pub(crate) fn get_node_phandle_prop_by_idx(node_idx: usize) -> Option<u32> {
    find_node_u32_sized_prop_by_name(node_idx, b"phandle")
        .or_else(|| find_node_u32_sized_prop_by_name(node_idx, b"linux,phandle"))
}

/// Returns the index of the node that holds the given phandle.
pub(crate) fn find_node_by_phandle_prop(value: u32) -> Option<usize> {
    (0..nodes().len()).find(|&node_idx| get_node_phandle_prop_by_idx(node_idx) == Some(value))
}

/// Returns the #interrupt-cells of a node, 1 when unset.
pub(crate) fn interrupt_cells(node_idx: usize) -> Option<usize> {
    get_node_from_arena_by_idx(node_idx)?;
    Some(find_node_u32_sized_prop_by_name(node_idx, b"#interrupt-cells").map_or(1, |v| v as usize))
}

/// Returns one entry of an interrupts-extended property as
/// `(phandle, args[0])`.
///
/// Each entry starts with a phandle, followed by the specifier cells of
/// the controller that the phandle points to. The specifier width is
/// that controller's #interrupt-cells. args[0] is the first specifier
/// cell.
///
/// None when the entry is absent, truncated, or the phandle is
/// dangling.
pub(crate) fn interrupts_extended(node_idx: usize, index: usize) -> Option<(u32, u32)> {
    let v = get_node_prop_value_by_name(node_idx, b"interrupts-extended")?;
    let mut off = 0usize;
    for i in 0..=index {
        let ph = utils::read_be_u32_from_bytes(&v[off..])?;
        let cells = interrupt_cells(find_node_by_phandle_prop(ph)?)?;
        let end = off.checked_add(4 + cells * 4)?;
        if end > v.len() {
            return None;
        }
        if i == index {
            return Some((ph, utils::read_be_u32_from_bytes(&v[off + 4..])?));
        }
        off = end;
    }
    None
}

/// Returns args[0] of one entry of an interrupts property.
///
/// The controller is the interrupt-parent of the node, or the nearest
/// ancestor with an interrupt-controller property. The entry width is
/// the #interrupt-cells of that controller.
pub(crate) fn get_node_interrupt_by_idx(node_idx: usize, inter_idx: usize) -> Option<u32> {
    let interrupt_controller = find_interrupt_controller_amongst_parent_nodes(node_idx)?;
    let cells = interrupt_cells(interrupt_controller)?;
    if cells == 0 {
        return None;
    }
    let v = get_node_prop_value_by_name(node_idx, b"interrupts")?;
    let off = inter_idx.checked_mul(cells * 4)?;
    let end = off.checked_add(cells * 4)?;
    if end > v.len() {
        return None;
    }
    utils::read_be_u32_from_bytes(&v[off..])
}

fn find_interrupt_controller_amongst_parent_nodes(node_idx: usize) -> Option<usize> {
    if let Some(value) = find_node_u32_sized_prop_by_name(node_idx, b"interrupt-parent") {
        return find_node_by_phandle_prop(value);
    }
    let mut cur = get_parent_by_node_idx(node_idx);
    while let Some(p) = cur {
        if get_node_prop_value_by_name(p, b"interrupt-controller").is_some() {
            return Some(p);
        }
        cur = get_parent_by_node_idx(p);
    }
    None
}

pub(crate) fn fdt_total_size() -> u32 {
    unsafe { FDT_TOTAL_SIZE }
}

pub(crate) fn get_reserved() -> &'static [Resource] {
    get_arena().reserved
}

pub(crate) fn get_node_idx_by_prop_name_and_val(
    prop_name: &[u8],
    prop_val: &[u8],
) -> Option<usize> {
    (0..nodes().len()).find(|&node_idx| {
        get_node_prop_value_by_name(node_idx, prop_name)
            .is_some_and(|v| v.split(|&b| b == 0).any(|s| s == prop_val))
    })
}
