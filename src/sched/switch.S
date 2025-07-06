.globl switch_to
switch_to:
  push rbp 
  push rbx
  push r12 
  push r13 
  push r14 
  push r15 

  mov [rdi], rsp        // store the old rsp to the first argument.
  mov rsp, rsi          // switch stack to the second argument.  
 
  pop r15 
  pop r14 
  pop r13
  pop r12 
  pop rbx 
  pop rbp 
 
  ret
