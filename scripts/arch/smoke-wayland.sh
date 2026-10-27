#!/usr/bin/env bash
set -euo pipefail
mkdir -p artifacts
export XDG_RUNTIME_DIR
XDG_RUNTIME_DIR=$(mktemp -d)
chmod 700 "$XDG_RUNTIME_DIR"
export WAYLAND_DISPLAY=s3-studio-test
app_pid=''
compositor_pid=''
cleanup() {
    if [[ -n "$app_pid" ]]; then kill "$app_pid" 2>/dev/null || true; fi
    if [[ -n "$compositor_pid" ]]; then kill "$compositor_pid" 2>/dev/null || true; fi
    rm -rf "$XDG_RUNTIME_DIR"
}
trap cleanup EXIT
# A nested desktop supplies wl_seat (headless Weston has no input devices).
# Only the compositor uses X11; the application below must use native Wayland.
weston --backend=x11 --renderer=pixman --socket="$WAYLAND_DISPLAY" --idle-time=0 > artifacts/omarchy-compositor.log 2>&1 &
compositor_pid=$!
for attempt in {1..30}; do
    [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] && break
    kill -0 "$compositor_pid" 2>/dev/null || { cat artifacts/omarchy-compositor.log; exit 1; }
    sleep 0.5
done
test -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY"
unset DISPLAY
WAYLAND_DEBUG=1 /usr/bin/s3-studio --preview > artifacts/omarchy-wayland.log 2>&1 &
app_pid=$!
for attempt in {1..60}; do
    kill -0 "$app_pid" 2>/dev/null || { cat artifacts/omarchy-wayland.log; exit 1; }
    if grep -q 'set_title("S3 Studio")' artifacts/omarchy-wayland.log &&
       grep -q 'xdg_surface.*ack_configure' artifacts/omarchy-wayland.log &&
       grep -q 'wl_surface.*attach(.*wl_buffer' artifacts/omarchy-wayland.log; then
        sleep 2
        kill -0 "$app_pid"
        echo 'Native Wayland window configured and rendered a buffer.'
        exit 0
    fi
    sleep 0.5
done
cat artifacts/omarchy-wayland.log
echo 'No rendered S3 Studio Wayland window within 30 seconds.' >&2
exit 1
