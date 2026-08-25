set -e

# `{chost}-emerge` reads PORTAGE_CONFIGROOT=/usr/${CHOST}, so target settings
# go in the crossdev prefix, not /target.
chost="${CROSS_COMPILE%-}"
cross="/usr/${chost}/etc/portage"

# panfrost alone.  The arm profile defaults VIDEO_CARDS to "exynos fbdev omap",
# but mesa has no video_cards_exynos flag at all: scanout comes from kmsro,
# which meson turns on automatically for any DRM gallium driver.  "exynos" only
# ever selected the dead x11-drivers/xf86-video-exynos DDX.
grep -q '^VIDEO_CARDS=' "${cross}/make.conf" ||
    echo 'VIDEO_CARDS="panfrost"' >> "${cross}/make.conf"

# mesa pulls libglvnd[X] for GLX.
mkdir -p "${cross}/package.use"
echo 'media-libs/libglvnd X' > "${cross}/package.use/mesa"

# mesa_clc builds for CBUILD, so the card selection is repeated on the host.
mkdir -p /etc/portage/package.use
echo 'dev-util/mesa_clc video_cards_panfrost' > /etc/portage/package.use/mesa

# eapply_user reads patches from PORTAGE_CONFIGROOT, so they go in the prefix
# too.  gallivm names llvm::StringMapIterator, which LLVM 22 removed, inside a
# DETECT_ARCH_ARM block: mesa 26.1.x fails to build for 32-bit ARM and only for
# 32-bit ARM.  Fixed in mesa main, not backported.
mkdir -p "${cross}/patches/media-libs/mesa"
cp /scripts/boards/odroid-xu4/patches/mesa-arm-llvm22-stringmap.patch \
   "${cross}/patches/media-libs/mesa/"
