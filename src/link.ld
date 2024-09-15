ENTRY(_start)

SECTIONS {
    . = 2M;

    .boot : {
		KEEP(*(.multiboot2_header))
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
