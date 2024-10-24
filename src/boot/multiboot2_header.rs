#[link_section = ".bootstrap.multiboot2_header"]
#[no_mangle]
#[used]
static MULTIBOOT2_HEADER: Header = Header {
    magic: MAGIC,
    architecture: ARCHITECTURE,
    header_length: HEADER_LENGTH,
    checksum: CHECKSUM,
    end_tag: END_TAG,
};

/// Magic value as specified in multiboot2.
const MAGIC: u32 = 0xE85250D6;
/// Specifies the CPU instruction set architecture. 
/// 
/// 0 for i386, 4 for 32-bit MIPS.
const ARCHITECTURE: u32 = 0;
const HEADER_LENGTH: u32 = core::mem::size_of::<Header>() as u32;
const CHECKSUM: u32 = !(MAGIC + ARCHITECTURE + HEADER_LENGTH) + 1;
const END_TAG: EndTag = EndTag {
    typ: 0, 
    flags: 0, 
    size: 8
};

#[repr(C)]
#[repr(align(8))]
struct Header {
    magic: u32,
    architecture: u32,
    header_length: u32,
    checksum: u32,
    end_tag: EndTag,
}


#[repr(C)]
#[repr(align(8))]
struct EndTag {
    typ: u16,
    flags: u16,
    size: u32,
}

