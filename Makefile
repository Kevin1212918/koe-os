koe-os.iso: $(wildcard src/*)
	cargo build 
	cp target/x86_64-unknown-none/debug/koe-os iso/boot
	tar --format=ustar -cvf iso/boot/initrd initrd
	grub-mkrescue -o koe-os.iso iso || grub2-mkrescue -o koe-os.iso iso
