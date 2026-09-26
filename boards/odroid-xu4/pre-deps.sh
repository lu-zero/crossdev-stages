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
