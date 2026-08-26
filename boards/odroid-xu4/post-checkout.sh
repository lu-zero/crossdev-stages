set -e

# Hardkernel's tree is only ever used for the three Samsung-signed blobs in
# sd_fuse/; the 2017 U-Boot sources next to them are ignored.  Shallow
# single-branch clone, no submodules.
rm -rf /build/exynos-blobs
git clone --depth=1 --single-branch \
    --branch "${EXYNOS_BLOB_TAG}" "${EXYNOS_BLOB_REPO}" /build/exynos-blobs

for blob in bl1.bin.hardkernel bl2.bin.hardkernel.720k_uboot tzsw.bin.hardkernel; do
    if [ ! -f "/build/exynos-blobs/sd_fuse/${blob}" ]; then
        echo "Error: missing signed blob ${blob}"
        exit 1
    fi
done
