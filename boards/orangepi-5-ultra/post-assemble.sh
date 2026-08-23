set -e

# Firmware, paths preserved: panthor asks for arm/mali/arch<N>.<M>/mali_csffw.bin,
# and the wireless drivers are just as picky about their subdirectory.
for dir in "${FIRMWARE_DIRS[@]}"; do
    [ -d "/build/firmware/${dir}" ] || { echo "Error: no firmware dir ${dir}"; exit 1; }
    mkdir -p "/build/gen/root/lib/firmware/${dir}"
    cp -a "/build/firmware/${dir}/." "/build/gen/root/lib/firmware/${dir}/"
done

# The stage3 fstab is comments only, so /boot never mounts and a kernel update
# would land in the rootfs copy instead of the partition u-boot reads.
cat >> /build/gen/root/etc/fstab <<FSTAB
PARTUUID=${BOOT_PART_UUID_2}  /      ext4  defaults,noatime  0 1
PARTUUID=${BOOT_PART_UUID_1}  /boot  ext4  defaults,noatime  0 2
FSTAB
