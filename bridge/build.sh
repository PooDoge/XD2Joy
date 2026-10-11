#!/bin/bash
# Builds the bridge firmware. Fetches TinyUSB (pinned) on first run, and for
# the RP2040 also the Pico SDK.
#   usage: bash bridge/build.sh [nucleo|xiao-rp2040]     (default: nucleo)
#   needs: arm-none-eabi-gcc, cmake, ninja, python3, git
#   output: bridge/out/<target>/xd2joy_bridge.{bin,uf2,...}
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
TINYUSB_REF=3ab3f9209a27eac15c464e31caec355c7b06b5cb
PICO_SDK_REF=2.1.1
TUSB="${TINYUSB_PATH:-$HERE/.deps/tinyusb}"
TARGET="${1:-nucleo}"

case "$TARGET" in
  nucleo)      BOARD=stm32f446nucleo;   FAMILY=stm32f4 ;;
  xiao-rp2040) BOARD=seeed_xiao_rp2040; FAMILY=rp2040 ;;
  *) echo "unknown target '$TARGET' (use nucleo or xiao-rp2040)" >&2; exit 2 ;;
esac

if [ ! -d "$TUSB/.git" ]; then
  mkdir -p "$(dirname "$TUSB")"
  git init -q "$TUSB"
  git -C "$TUSB" remote add origin https://github.com/hathach/tinyusb.git
fi
if [ "$(git -C "$TUSB" rev-parse HEAD 2>/dev/null || true)" != "$TINYUSB_REF" ]; then
  git -C "$TUSB" fetch -q --depth 1 origin "$TINYUSB_REF"
  git -C "$TUSB" checkout -q FETCH_HEAD
fi

EXTRA=()
if [ "$FAMILY" = stm32f4 ]; then
  if [ ! -d "$TUSB/hw/mcu/st/cmsis_device_f4/Include" ]; then
    python3 "$TUSB/tools/get_deps.py" stm32f4
  fi
else
  if [ ! -f "$TUSB/hw/mcu/raspberry_pi/Pico-PIO-USB/src/pio_usb.c" ]; then
    python3 "$TUSB/tools/get_deps.py" rp2040
  fi
  SDK="${PICO_SDK_PATH:-$HERE/.deps/pico-sdk}"
  if [ ! -d "$SDK/src" ]; then
    git clone -q --depth 1 -b "$PICO_SDK_REF" \
      https://github.com/raspberrypi/pico-sdk.git "$SDK"
  fi
  EXTRA+=(-DPICO_SDK_PATH="$SDK")
fi

# TinyUSB only looks for boards inside its own tree
ln -sfn "$HERE/boards/$BOARD" "$TUSB/hw/bsp/$FAMILY/boards/$BOARD"

BUILD="$HERE/build/$TARGET"
cmake -S "$HERE" -B "$BUILD" -G Ninja \
  -DTINYUSB_PATH="$TUSB" -DBOARD="$BOARD" -DCMAKE_BUILD_TYPE=MinSizeRel \
  "${EXTRA[@]}"
cmake --build "$BUILD"

OUT="$HERE/out/$TARGET"
mkdir -p "$OUT"
for ext in elf bin hex uf2; do
  if [ -f "$BUILD/xd2joy_bridge.$ext" ]; then
    cp "$BUILD/xd2joy_bridge.$ext" "$OUT/"
  fi
done
if [ "$TARGET" = nucleo ]; then
  echo "-> bridge/out/nucleo/xd2joy_bridge.bin (drag onto the NOD_F446RE drive)"
else
  echo "-> bridge/out/xiao-rp2040/xd2joy_bridge.uf2 (drag onto the RPI-RP2 drive)"
fi
