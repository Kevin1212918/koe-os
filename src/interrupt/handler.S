.extern exception_handler

.macro EXN_ENTRY int_vec
isr_\int_vec:
  sub rsp, 0x8
  push \int_vec

  jmp _do_exception_handler
.section .data
.quad isr_\int_vec
.section .text
.endm

.macro EXN_ERRNO_ENTRY int_vec
isr_\int_vec:
  push \int_vec

  jmp _do_exception_handler
.section .data
.quad isr_\int_vec
.section .text
.endm

.macro ISR_PADDING cnt
.section .data
.skip 8 * \cnt
.section .text
.endm

_do_exception_handler: 
  push rax
  push rdi
  push rsi
  push rdx
  push rcx
  push r8
  push r9
  push r10
  push r11
  sub rsp, 0x8 

  mov rdi, [rsp+0x50]
  lea rsi, [rsp+0x58]
  
  call exception_handler

  add rsp, 0x8
  pop r11
  pop r10
  pop r9
  pop r8
  pop rcx
  pop rdx
  pop rsi
  pop rdi
  pop rax
  
  add rsp, 0x10
  iretq

.macro IRQ_ENTRY int_vec
isr_\int_vec:
  sub rsp, 0x8
  push \int_vec

  jmp _do_irq_handler
.section .data
.quad isr_\int_vec
.section .text


.endm
_do_irq_handler: 
  push rax
  push rdi
  push rsi
  push rdx
  push rcx
  push r8
  push r9
  push r10
  push r11
  sub rsp, 0x8

  mov rdi, [rsp+0x50]
  lea rsi, [rsp+0x58]
  
  call irq_handler

  add rsp, 0x8
  pop r11
  pop r10
  pop r9
  pop r8
  pop rcx
  pop rdx
  pop rsi
  pop rdi
  pop rax
  
  add rsp, 0x10
  iretq



// Initializing a table holding all isr entrys
.globl ISR_TABLE
.section .data
.align 8
ISR_TABLE:
.section .text 

EXN_ENTRY 0
EXN_ENTRY 1
EXN_ENTRY 2
EXN_ENTRY 3
EXN_ENTRY 4
EXN_ENTRY 5
EXN_ENTRY 6
EXN_ENTRY 7
EXN_ERRNO_ENTRY 8
EXN_ENTRY 9
EXN_ERRNO_ENTRY 10
EXN_ERRNO_ENTRY 11
EXN_ERRNO_ENTRY 12
EXN_ERRNO_ENTRY 13
EXN_ERRNO_ENTRY 14
ISR_PADDING 1 // exn 15 is reserved
EXN_ENTRY 16
EXN_ERRNO_ENTRY 17
EXN_ENTRY 18
EXN_ENTRY 19
EXN_ENTRY 20
EXN_ERRNO_ENTRY 21

ISR_PADDING 10 // padding from 22..32

IRQ_ENTRY 32
IRQ_ENTRY 33
IRQ_ENTRY 34
IRQ_ENTRY 35
IRQ_ENTRY 36
IRQ_ENTRY 37
IRQ_ENTRY 38
IRQ_ENTRY 39
IRQ_ENTRY 40
IRQ_ENTRY 41
IRQ_ENTRY 42
IRQ_ENTRY 43
IRQ_ENTRY 44
IRQ_ENTRY 45
IRQ_ENTRY 46
IRQ_ENTRY 47
