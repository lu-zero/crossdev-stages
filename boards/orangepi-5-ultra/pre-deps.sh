set -e

# `{chost}-emerge` reads PORTAGE_CONFIGROOT=/usr/${CHOST}, so target settings
# go in the crossdev prefix, not /target.
chost="${CROSS_COMPILE%-}"
cross="/usr/${chost}/etc/portage"

grep -q '^VIDEO_CARDS=' "${cross}/make.conf" ||
    echo 'VIDEO_CARDS="panfrost"' >> "${cross}/make.conf"

# mesa pulls libglvnd[X] for GLX.
mkdir -p "${cross}/package.use"
echo 'media-libs/libglvnd X' > "${cross}/package.use/mesa"

# mesa_clc builds for CBUILD, so the card selection is repeated on the host.
mkdir -p /etc/portage/package.use
echo 'dev-util/mesa_clc video_cards_panfrost' > /etc/portage/package.use/mesa
