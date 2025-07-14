.extern KERNEL_ENTRY_STACK_PTR

.globl do_syscall_entry
.globl do_syscall_exit
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
do_syscall_entry: 
  mov [KERNEL_ENTRY_STACK_PTR], rsp  // Put stack pointer on top of kernel entry stack
  mov rsp, KERNEL_ENTRY_STACK_PTR    // switch stack 

  push rcx                           // push return ip
  push r11                           // push calling rflags 

  mov rcx, r10                       // move arg4 to the System V specified register 
  
  // call RUST_HANDLER
do_syscall_exit:
  pop r11
  pop rcx

  pop rsp
  sysretq

.globl set_syscall_msrs
set_syscall_msrs:
  mov rcx, 0xc0000082                // Write syscall entry address to IA32_LSTAR MSR
  mov rax, do_syscall_entry
  mov rdx, do_syscall_entry >> 32
  wrmsr

  mov rcx, 0xc0000084
  mov rax, 1 << 9                    // Mask out interrupt flag 
  mov rdx, 0 
  wrmsr
  ret
