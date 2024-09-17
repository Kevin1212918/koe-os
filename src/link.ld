ENTRY(_start)

SECTIONS {
    . = 2M;

    .bootstrap : {
		KEEP(*(.bootstrap.multiboot2_header))
		*(.bootstrap.text)
		*(.bootstrap.rodata)
		*(.bootstrap.bss)
    }

	. = 0xFFFF800000000000;
	_kernel_start_vma = .;

    .text ALIGN (4K) : AT (ADDR (.text) - _kernel_start_vma) {
		*(.text)
	}

	.rodata ALIGN (4K) : AT (ADDR (.rodata) - _kernel_start_vma) {
		*(.rodata)
	}

	.data ALIGN (4K) : AT (ADDR (.data) - _kernel_start_vma) {
		*(.data)
	}

	.bss ALIGN (4K) : AT (ADDR (.bss) - _kernel_start_vma) {
		*(COMMON)
		*(.bss)
	}

	_kernel_end_vma = .;
}