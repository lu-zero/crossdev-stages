setenv bootargs "console=ttyS0,115200 root=/dev/mmcblk0p2 rw rootwait rootfstype=ext4"

load mmc 0:1 ${kernel_addr_r} /Image
load mmc 0:1 ${fdt_addr_r} /x1_orangepi-rv2.dtb
load mmc 0:1 ${ramdisk_addr_r} /uInitrd

# This is important
fdt resize 65536

booti ${kernel_addr_r} ${ramdisk_addr_r} ${fdt_addr_r}
