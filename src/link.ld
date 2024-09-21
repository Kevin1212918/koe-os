ENTRY(_start)
SECTIONS {
	_KERNEL_VMA_OFFSET = 0xFFFFFFFF80000000;

    . = 0x200000;
	_BOOTSTRAP_START_VMA = .;
	_BOOTSTRAP_START_LMA = .;

    .bootstrap.multiboot2_header : {
		KEEP(*(.bootstrap.multiboot2_header))
    }
	.bootstrap.text : {
		*(.bootstrap.text)
	}
	.bootstrap.data : {
		*(.bootstrap.data)
	}
	.bootstrap.bss : {
		*(.bootstrap.bss)
	}

	_BOOTSTRAP_END_VMA = .;

	_KERNEL_START_LMA = .;
	. += _KERNEL_VMA_OFFSET;
	_KERNEL_START_VMA = .;

    .text ALIGN (4K) : AT (ADDR (.text) - _KERNEL_VMA_OFFSET) {
		*(.text .text.*)
	}

	.rodata ALIGN (4K) : AT (ADDR (.rodata) - _KERNEL_VMA_OFFSET) {
		*(.rodata .rodata.*)
	}

	.data ALIGN (4K) : AT (ADDR (.data) - _KERNEL_VMA_OFFSET) {
		*(.data .data.*)
	}

	.bss ALIGN (4K) : AT (ADDR (.bss) - _KERNEL_VMA_OFFSET) {
		*(COMMON)
		*(.bss .bss.*)
	}

	_KERNEL_END_VMA = .;
}