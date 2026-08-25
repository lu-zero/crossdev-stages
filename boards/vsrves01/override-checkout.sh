#!/bin/bash
set -e

# Kernel: standard cached_clone from stable/linux.git (the framework does
# this automatically when KERNEL_REPO is set).  We just need to also pull
# the vendor SDK bits not in any git repo: rvlbne.bin (RV bootloader for
# VSDSP6), SYSR/*.dr3, startup.txt, Welcome.txt etc.  The kernel itself,
# DTB, and Ethernet driver are now all in-tree via patches/.

if [ ! -d /build/firmware/vsrv_root ]; then
    mkdir -p /build/firmware
    # vsrv_root_<DATE>.zip — System Root Files from VSDSP Forum (t=3242).
    # Pinned filename below tracks the 2026-05-11 release; bump when newer.
    # If the user pre-fetched it into /scripts/boards/vsrves01/firmware/
    # (manual download because the forum URL needs login / changes), copy
    # from there.
    if [ -f /scripts/boards/vsrves01/firmware/vsrv_root_260511.zip ]; then
        cp /scripts/boards/vsrves01/firmware/vsrv_root_260511.zip \
            /build/firmware/vsrv_root_260511.zip
    else
        echo "ERROR: vsrv_root_260511.zip not found.  Download manually from"
        echo "       https://www.vsdsp-forum.com/phpbb/viewtopic.php?t=3242"
        echo "       and place under boards/vsrves01/firmware/"
        echo "       See boards/vsrves01/firmware/README.md."
        exit 1
    fi
    # Pin: rvlbne.bin embeds fixed _ddrBlob offsets; a different forum
    # re-upload of the zip can shift catboard.dtb property layout and
    # break the in-RAM patch.
    if [ -f /scripts/boards/vsrves01/firmware/vsrv_root_260511.zip.sha256 ]; then
        ( cd /build/firmware \
          && sha256sum -c /scripts/boards/vsrves01/firmware/vsrv_root_260511.zip.sha256 )
    else
        echo "ERROR: vsrv_root_260511.zip.sha256 not found alongside the zip."
        echo "       Generate with: sha256sum vsrv_root_260511.zip > vsrv_root_260511.zip.sha256"
        exit 1
    fi
    cd /build/firmware
    unzip -o vsrv_root_260511.zip -d vsrv_root_tmp
    # Zip nests its contents under vsrv_root_260511/; flatten.
    mv vsrv_root_tmp/vsrv_root_260511 vsrv_root
    rmdir vsrv_root_tmp
fi

# Linux kernel clone — same path the default_checkout would have taken.
if [ ! -d /build/linux ]; then
    git clone --depth 1 --branch "${KERNEL_TAG}" "${KERNEL_REPO}" /build/linux
fi
