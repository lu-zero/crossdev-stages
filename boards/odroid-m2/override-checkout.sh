set -e

clone_cached() {
    local repo=$1 tag=$2 dest=$3 name=$4
    local cache="/cache/sources/${name}.git"
    if [ -d "$cache" ]; then
        git -C "$cache" fetch --prune 2>/dev/null || true
    else
        git clone --bare "$repo" "$cache"
    fi
    git clone --reference "$cache" --depth=1 --branch "$tag" "$repo" "$dest"
}

clone_cached "${RKBIN_REPO}" master /build/rkbin rkbin
clone_cached "${TFA_REPO}" "${TFA_TAG}" /build/tfa arm-trusted-firmware
clone_cached "${U_BOOT_REPO}" "${U_BOOT_TAG}" /build/u-boot u-boot-upstream
clone_cached "${KERNEL_REPO}" "${KERNEL_TAG}" /build/linux linux-mainline
