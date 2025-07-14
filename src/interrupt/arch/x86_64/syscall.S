.extern KERNEL_ENTRY_STACK_PTR

// User space syscall kernel entry. 
// Entered from the syscall instruction. Interrupt is disabled upon entry.
// 
// - rax: syscall vector 
// - rdi: arg1
// - rsi: arg2 
// - rdx: arg3
// - r10: arg4 
// - r8:  arg5
// - r9:  arg6
//
// - r11: calling rflags 
// - rcx: return ip
// - rsp: calling stack
do_syscall: 
  mov [KERNEL_ENTRY_STACK_PTR], rsp  // Put stack pointer on top of kernel entry stack
  mov rsp, KERNEL_ENTRY_STACK_PTR    // switch stack 

  push rcx                           // push return ip
  push r11                           // push calling rflags 

  mov rcx, r10                       // move arg4 to the System V specified register 
  
  call RUST_HANDLER

  pop r11
  pop rcx

  pop rsp
  sysret
