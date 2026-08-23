set -e

# `{chost}-emerge` reads PORTAGE_CONFIGROOT=/usr/${CHOST}, so target settings
# go in the crossdev prefix, not /target.
chost="${CROSS_COMPILE%-}"
cross="/usr/${chost}/etc/portage"

grep -q '^VIDEO_CARDS=' "${cross}/make.conf" ||
    echo 'VIDEO_CARDS="panfrost"' >> "${cross}/make.conf"

# No X server on this board.  zink runs GL on top of panvk, which is the path
# a wlroots compositor can actually accelerate on Mali-G610.
#
# Appended to USE rather than assigned: crossdev already wrote USE="${ARCH}"
# here, so testing for the variable at all just skips this silently and leaves
# every package on the profile default -- which is how mpv ended up asking for
# vulkan-loader[X].
grep -q 'crossdev-stages USE' "${cross}/make.conf" || cat >> "${cross}/make.conf" <<'USEEOF'
# crossdev-stages USE
USE="${USE} -X wayland vulkan zink alsa pipewire screencast"
USEEOF

mkdir -p "${cross}/package.use"

# mesa pulls libglvnd[X] for GLX.
echo 'media-libs/libglvnd X' > "${cross}/package.use/mesa"

# x264 forces gpl in REQUIRED_USE.  v4l for the HDMI receiver, srt to ship the
# stream, opus for the audio that goes with it.
echo 'media-video/ffmpeg gpl x264 v4l alsa opus srt' > "${cross}/package.use/ffmpeg"

# alsa is off by default here and compositor/textoverlay live in -base.
echo 'media-libs/gst-plugins-base alsa pango gles2 egl' > "${cross}/package.use/gstreamer"

# modetest, which is how the HDMI output modes get read, ships only with tools.
echo 'x11-libs/libdrm tools' > "${cross}/package.use/libdrm"

# mesa_clc builds for CBUILD, so the card selection is repeated on the host.
mkdir -p /etc/portage/package.use
echo 'dev-util/mesa_clc video_cards_panfrost' > /etc/portage/package.use/mesa
