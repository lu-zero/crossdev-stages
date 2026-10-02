set -e

# Firmware, paths preserved: r8152 asks for rtl_nic/rtl8153a-3.fw by name.
for dir in "${FIRMWARE_DIRS[@]}"; do
    [ -d "/build/firmware/${dir}" ] || { echo "Error: no firmware dir ${dir}"; exit 1; }
    mkdir -p "/build/gen/root/lib/firmware/${dir}"
    cp -a "/build/firmware/${dir}/." "/build/gen/root/lib/firmware/${dir}/"
done

# Mainline U-Boot's odroid-xu3 target keeps CONFIG_DISTRO_DEFAULTS, whose
# BOOT_TARGET_DEVICES walks mmc2 (the SD slot), mmc1 and mmc0 looking for
# extlinux/extlinux.conf.  Hardkernel's boot.ini is a fork-only feature and is
# not needed.  DISTRO_DEFAULTS also pulls in CMD_BOOTZ on 32-bit ARM, so a raw
# zImage boots without being wrapped as a uImage.
mkdir -p /build/gen/boot/extlinux
kver=$(ls /build/gen/root/lib/modules/ | head -1)
[ -z "$kver" ] && { echo 'Error: no kernel modules found'; exit 1; }
cat > /build/gen/boot/extlinux/extlinux.conf << EXTEOF
DEFAULT gentoo
TIMEOUT 30
LABEL gentoo
    MENU LABEL Gentoo Linux
    LINUX /${BOOT_KERNEL_NAME}
    FDT /exynos5422-odroidxu4.dtb
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=tty1 console=${BOOT_CONSOLE} earlycon
LABEL gentoo-drm-debug
    MENU LABEL Gentoo Linux (verbose display bring-up)
    LINUX /${BOOT_KERNEL_NAME}
    FDT /exynos5422-odroidxu4.dtb
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=tty1 console=${BOOT_CONSOLE} earlycon drm.debug=0x1f log_buf_len=4M ignore_loglevel loglevel=8
EXTEOF

# The stage3 fstab is comments only, so /boot never mounts and kernel updates
# would land in the rootfs copy instead of the partition u-boot reads.
# Same PARTUUID scheme as root: index-agnostic, so the card boots the same
# whether or not an eMMC module is fitted.
cat >> /build/gen/root/etc/fstab <<FSTAB
PARTUUID=${BOOT_DISK_ID}-02  /      ext4  defaults,noatime  0 1
PARTUUID=${BOOT_DISK_ID}-01  /boot  ext4  defaults,noatime  0 2
FSTAB

# Docker install gotchas from real hardware (see README's Known gotchas).
# These two packages hardcode CGO_ENABLED=0, so patch it via a bashrc hook.
cat >> /build/gen/root/etc/portage/bashrc <<'EOF'
post_src_prepare() {
	case "${CATEGORY}/${PN}" in
		dev-go/go-md2man|app-containers/containerd)
			sed -i 's/CGO_ENABLED=0/CGO_ENABLED=1/' Makefile
			;;
	esac
}
EOF

# containerd needs network for `go mod download`, blocked by network-sandbox.
# SHIM_CGO_ENABLED is a `?=` var, so an env var works directly (no sed).
mkdir -p /build/gen/root/etc/portage/env
cat > /build/gen/root/etc/portage/env/allow-net <<'EOF'
FEATURES="${FEATURES} -network-sandbox"
SHIM_CGO_ENABLED=1
EOF
echo 'app-containers/containerd allow-net' >> /build/gen/root/etc/portage/package.env

# docker-cli's own ebuild hardcodes CGO_ENABLED=0 for its manpages target
# since 29.7.2 (no sed/env fix works against that); docker-29.8.1+ blocks
# older docker-cli, so both are capped at the last good pair.
mkdir -p /build/gen/root/etc/portage/package.mask
cat > /build/gen/root/etc/portage/package.mask/docker-cli <<'EOF'
>app-containers/docker-cli-29.5.2
>app-containers/docker-29.5.2
EOF
