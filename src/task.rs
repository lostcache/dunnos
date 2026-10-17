use core::{arch::global_asm, cell::SyncUnsafeCell, mem::offset_of};

static TASKS: SyncUnsafeCell<[Task; MAX_TASKS]> =
    SyncUnsafeCell::new([const { Task::empty() }; MAX_TASKS]);
static CURRENT: SyncUnsafeCell<usize> = SyncUnsafeCell::new(0);

#[unsafe(no_mangle)]
pub(crate) static CURRENT_KSTACK_TOP: SyncUnsafeCell<usize> = SyncUnsafeCell::new(0);

global_asm!(
    r#"
    .align 2
    .global switch_to
switch_to:
    sd ra, {ra}(a0)
    sd sp, {sp}(a0)
    sd s0, {s0}(a0)
    sd s1, {s1}(a0)
    sd s2, {s2}(a0)
    sd s3, {s3}(a0)
    sd s4, {s4}(a0)
    sd s5, {s5}(a0)
    sd s6, {s6}(a0)
    sd s7, {s7}(a0)
    sd s8, {s8}(a0)
    sd s9, {s9}(a0)
    sd s10, {s10}(a0)
    sd s11, {s11}(a0)

    ld ra, {ra}(a1)
    ld sp, {sp}(a1)
    ld s0, {s0}(a1)
    ld s1, {s1}(a1)
    ld s2, {s2}(a1)
    ld s3, {s3}(a1)
    ld s4, {s4}(a1)
    ld s5, {s5}(a1)
    ld s6, {s6}(a1)
    ld s7, {s7}(a1)
    ld s8, {s8}(a1)
    ld s9, {s9}(a1)
    ld s10, {s10}(a1)
    ld s11, {s11}(a1)

    ret
    "#,
    ra = const offset_of!(TaskContext, ra),
    sp = const offset_of!(TaskContext, sp),
    s0 = const offset_of!(TaskContext, s0),
    s1 = const offset_of!(TaskContext, s1),
    s2 = const offset_of!(TaskContext, s2),
    s3 = const offset_of!(TaskContext, s3),
    s4 = const offset_of!(TaskContext, s4),
    s5 = const offset_of!(TaskContext, s5),
    s6 = const offset_of!(TaskContext, s6),
    s7 = const offset_of!(TaskContext, s7),
    s8 = const offset_of!(TaskContext, s8),
    s9 = const offset_of!(TaskContext, s9),
    s10 = const offset_of!(TaskContext, s10),
    s11 = const offset_of!(TaskContext, s11),
);

unsafe extern "C" {
    fn switch_to(old: *mut TaskContext, new: *const TaskContext);
}

pub(crate) struct Task {
    ctx: TaskContext,
    kstack_top: usize,
    root_table: usize,
    pid: usize,
    asid: usize,
    user_frame: [usize; 8],
    state: TaskState,
}

#[repr(C)]
pub(crate) struct TaskContext {
    ra: usize,
    sp: usize,
    s0: usize,
    s1: usize,
    s2: usize,
    s3: usize,
    s4: usize,
    s5: usize,
    s6: usize,
    s7: usize,
    s8: usize,
    s9: usize,
    s10: usize,
    s11: usize,
}

#[derive(PartialEq)]
pub(crate) enum TaskState {
    Unused,
    Ready,
    Running,
    Dead,
}
