koe-os.iso: iso/boot/koe-os $(wildcard iso/boot/*)
	tar --format=ustar -cvf iso/boot/initrd initrd/
	grub-mkrescue -o koe-os.iso iso || grub2-mkrescue -o koe-os.iso iso

.PHONY: cargo
cargo: 
	cargo build

iso/boot/koe-os: cargo
	cp target/x86_64-unknown-none/debug/koe-os iso/boot

