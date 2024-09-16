ENTRY(_start)

SECTIONS {
    . = 2M;

    .bootstrap : {
		KEEP(*(.bootstrap.multiboot2_header))
    }

    .text : {
		*(.text)
	}

	.rodata : {
		*(.rodata)
	}

	.data : {
		*(.data)
	}

	.bss : {
		*(COMMON)
		*(.bss)
	}
}
