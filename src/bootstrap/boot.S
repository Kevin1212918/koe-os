.code32
.set _KERNEL_OFFSET_VMA, 0xFFFFFFFF80000000

.section .bootstrap.data, "a", @progbits
.align 64
gdt:
    .quad 0
    .set gdt_code, . - gdt
    .quad (1<<43) | (1<<44) | (1<<47) | (1<<53) /* Code Segment */
gdt_ptr:
    .word . - gdt - 1
    .quad gdt

.section .bootstrap.bss, "aw", @nobits
.align 4096
pg_ml4_table:
    .skip 4096
pg_dir_ptr_table:
    .skip 4096
pg_dir_table:
    .skip 4096

.section .bss
.align 4096
stack_bottom:
    .skip 16384 # 16 KiB
stack_top:

.section .bootstrap.text, "ax"
.global _start
_start: 
    
    /* Check if loaded by multiboot2 compliant bootloader */
    cmp eax, 0x36d76289
    jne _start_no_multiboot2_err
    
    /* Load stack address into registers */
    .set stack_top_lma, stack_top - _KERNEL_OFFSET_VMA
    lea esp, stack_top_lma
    push 0

_check_cpuid:

    
    /* Check if CPUID is supported by attempting to flip the ID bit (bit 21)
       in the FLAGS register. If we can flip it, CPUID is available.

       Copy FLAGS in to EAX via stack */
    pushfd
    pop eax 

    /* Copy to ECX as well for comparing later on */
    mov ecx, eax

    /* Flip the ID bit */
    xor eax, 1 << 21

    /* Copy EAX to FLAGS via the stack */
    push eax 
    popfd
    
    /* Copy FLAGS back to EAX (with the flipped bit if CPUID is supported) */
    pushfd
    pop eax

    /* Restore FLAGS from the old version stored in ECX (i.e. flipping the
       ID bit back if it was ever flipped).*/
    push ecx
    popfd

    /* Compare EAX and ECX. If they are equal then that means the bit
       wasn't flipped, and CPUID isn't supported. */
    xor eax, ecx
    jz _start_no_cpuid_err 
    

_check_long_mode:
    /* test if extended processor info in available */
    mov eax, 0x80000000    /* implicit argument for cpuid */
    cpuid                  /* get highest supported argument */
    cmp eax, 0x80000001    /* it needs to be at least 0x80000001 */
    jb _start_no_long_mode_err

    /* extended info to test if long mode is available */
    mov eax, 0x80000001    /* for extended processor info */
    cpuid                  /* various feature bits in ecx and edx */
    test edx, 1 << 29      /* if the LM-bit is set in the D-register */
    jz _start_no_long_mode_err       /* it's not set, there is no long mode */
    
_enable_paging:
    /* Set first entry of pml4 */
    lea eax, pg_dir_ptr_table 
    or eax, 0b11
    mov [pg_ml4_table], eax

    /* Set last entry of pml4 */
    lea eax, pg_dir_ptr_table 
    or eax, 0b11
    mov [offset pg_ml4_table + 511 * 8], eax

    /* Set first entry of page directory ptr table*/
    lea eax, pg_dir_table
    or eax, 0b11
    mov [pg_dir_ptr_table], eax
    
    /* Set 511th entry of page directory ptr table*/
    lea eax, pg_dir_table
    or eax, 0b11
    mov [offset pg_dir_ptr_table + 510 * 8], eax

    /* Set first entry of page directory table */
    mov eax, 0 
    or eax, 0b10000011
    mov [pg_dir_table], eax
    
    /* Set second entry of page directory table */
    mov eax, 0x200000
    or eax, 0b10000011
    mov [pg_dir_table + 8], eax
    
    /* Set third entry of page directory table */
    mov eax, 0x400000
    or eax, 0b10000011
    mov [pg_dir_table + 16], eax

    /* Set fourth entry of page directory table */
    mov eax, 0x600000
    or eax, 0b10000011
    mov [pg_dir_table + 24], eax

    /* Set up CR3 */
    lea eax, pg_ml4_table
    mov cr3, eax

    /* Set up PAE */
    mov eax, cr4
    or eax, 0b100000
    mov cr4, eax

    /* Enable long mode */
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    /* Enable paging */
    mov eax, cr0
    or eax, 1 << 31
    mov cr0, eax

    /* Call rust kernel entry point */
    lgdt [gdt_ptr]
    ljmp offset gdt_code, offset _start_long_mode


_start_no_long_mode_err:
    mov al, '2'
    jmp _start_err
_start_no_cpuid_err:
    mov al, '1'
    jmp _start_err
_start_no_multiboot2_err:
    mov al, '0'
    jmp _start_err
_start_err:
    mov dword ptr [0xb8000], 0x4f524f45
    mov dword ptr [0xb8004], 0x4f3a4f52
    mov dword ptr [0xb8008], 0x4f204f20
    mov byte ptr [0xb800a], al
    hlt

.code64
.extern kmain
_start_long_mode:
    lea rsp, stack_top
    mov ax, 0
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    jmp kmain
