set -e

# `{chost}-emerge` reads PORTAGE_CONFIGROOT=/usr/${CHOST}, so target settings
# go in the crossdev prefix, not /target.
chost="${CROSS_COMPILE%-}"
cross="/usr/${chost}/etc/portage"

grep -q '^VIDEO_CARDS=' "${cross}/make.conf" ||
    echo 'VIDEO_CARDS="panfrost"' >> "${cross}/make.conf"

# No X server on this board.  zink runs GL on top of panvk, which is the path
# a wlroots compositor can actually accelerate on Mali-G610.
grep -q '^USE=' "${cross}/make.conf" ||
    echo 'USE="-X wayland vulkan zink alsa pipewire screencast"' >> "${cross}/make.conf"

mkdir -p "${cross}/package.use"

# mesa pulls libglvnd[X] for GLX.
echo 'media-libs/libglvnd X' > "${cross}/package.use/mesa"

# x264 forces gpl in REQUIRED_USE.  v4l for the HDMI receiver, srt to ship the
# stream, opus for the audio that goes with it.
echo 'media-video/ffmpeg gpl x264 v4l alsa opus srt' > "${cross}/package.use/ffmpeg"

# alsa is off by default here and compositor/textoverlay live in -base.
echo 'media-libs/gst-plugins-base alsa pango gles2 egl' > "${cross}/package.use/gstreamer"

# mesa_clc builds for CBUILD, so the card selection is repeated on the host.
mkdir -p /etc/portage/package.use
echo 'dev-util/mesa_clc video_cards_panfrost' > /etc/portage/package.use/mesa
