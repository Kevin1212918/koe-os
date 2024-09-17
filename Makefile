koe-os.iso: src/*
	rm -rf iso
	rm -f koe-os.iso

	mkdir -p iso/boot/grub

	cargo build 

	cp src/grub.cfg iso/boot/grub
	cp target/x86_64-unknown-none/debug/koe-os iso/boot

	grub-mkrescue -o koe-os.iso iso
	rm -rf iso
