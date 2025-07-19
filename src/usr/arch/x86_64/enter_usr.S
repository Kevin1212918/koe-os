.extern KERNEL_ENTRY_STACK_PTR
.extern do_syscall_exit

.globl enter_usr
enter_usr:
  cli
  mov rsp, [KERNEL_ENTRY_STACK_PTR] 
  
  push rdi 
  push rsi 
  push 1 << 9 | 3 << 12     // Set iopl to 3

  jmp do_syscall_exit

