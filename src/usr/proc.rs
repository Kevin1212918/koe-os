use crate::mem::X86_64MemoryMap;

pub type Pid = u32;
pub type Tid = u32;

struct X86_64ExecCxt {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    rsp: u64,
    rbp: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rip: u64,
    cs: u64,
    ds: u64,
    ss: u64,
    es: u64,
    fs: u64,
    gs: u64,
}
struct Tcb {
    id: Tid,
    exec_cxt: X86_64ExecCxt,
}
struct Pcb {
    id: Pid,
    mem_map: Option<X86_64MemoryMap>,
}
