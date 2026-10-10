#!/bin/bash
# Builds the bridge firmware. Fetches TinyUSB (pinned) on first run.
#   needs: arm-none-eabi-gcc, cmake, ninja, python3, git
#   output: bridge/out/xd2joy_bridge.{elf,bin,hex}
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
TINYUSB_REF=3ab3f9209a27eac15c464e31caec355c7b06b5cb
TUSB="${TINYUSB_PATH:-$HERE/.deps/tinyusb}"
BOARD=stm32f446nucleo

if [ ! -d "$TUSB/.git" ]; then
  mkdir -p "$(dirname "$TUSB")"
  git init -q "$TUSB"
  git -C "$TUSB" remote add origin https://github.com/hathach/tinyusb.git
fi
if [ "$(git -C "$TUSB" rev-parse HEAD 2>/dev/null || true)" != "$TINYUSB_REF" ]; then
  git -C "$TUSB" fetch -q --depth 1 origin "$TINYUSB_REF"
  git -C "$TUSB" checkout -q FETCH_HEAD
fi
if [ ! -d "$TUSB/hw/mcu/st/cmsis_device_f4/Include" ]; then
  python3 "$TUSB/tools/get_deps.py" stm32f4
fi

# TinyUSB only looks for boards inside its own tree
ln -sfn "$HERE/boards/$BOARD" "$TUSB/hw/bsp/stm32f4/boards/$BOARD"

cmake -S "$HERE" -B "$HERE/build" -G Ninja \
  -DTINYUSB_PATH="$TUSB" -DBOARD="$BOARD" -DCMAKE_BUILD_TYPE=MinSizeRel
cmake --build "$HERE/build"

mkdir -p "$HERE/out"
for ext in elf bin hex; do
  cp "$HERE/build/xd2joy_bridge.$ext" "$HERE/out/"
done
echo "-> bridge/out/xd2joy_bridge.bin (drag onto the NOD_F446RE drive)"
