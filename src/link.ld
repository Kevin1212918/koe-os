ENTRY(_start)

SECTIONS {
    . = 0x200000;

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

	. += 0xFFFFFFFF80000000;
	_kernel_start_vma = .;

    .text ALIGN (4K) : AT (ADDR (.text) - 0xFFFFFFFF80000000) {
		*(.text .text.*)
	}

	.rodata ALIGN (4K) : AT (ADDR (.rodata) - 0xFFFFFFFF80000000) {
		*(.rodata .rodata.*)
	}

	.data ALIGN (4K) : AT (ADDR (.data) - 0xFFFFFFFF80000000) {
		*(.data .data.*)
	}

	.bss ALIGN (4K) : AT (ADDR (.bss) - 0xFFFFFFFF80000000) {
		*(COMMON)
		*(.bss .bss.*)
	}

}