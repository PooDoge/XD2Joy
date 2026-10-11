cmake_minimum_required(VERSION 3.20)

# Build with ./build.sh, which fetches TinyUSB and links the board in.
if (NOT DEFINED TINYUSB_PATH)
  message(FATAL_ERROR "Set -DTINYUSB_PATH=<tinyusb checkout> (build.sh does this)")
endif ()
if (NOT DEFINED BOARD)
  set(BOARD stm32f446nucleo)
endif ()

include(${TINYUSB_PATH}/hw/bsp/family_support.cmake)

project(xd2joy_bridge C CXX ASM)

family_initialize_project(${PROJECT_NAME} ${CMAKE_CURRENT_LIST_DIR})

add_executable(${PROJECT_NAME})

target_sources(${PROJECT_NAME} PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/src/main.c
  ${CMAKE_CURRENT_SOURCE_DIR}/src/hid_parse.c
  ${CMAKE_CURRENT_SOURCE_DIR}/src/input.c
  )
if (FAMILY STREQUAL "rp2040")
  target_sources(${PROJECT_NAME} PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/src/link_rp2040.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/plat_rp2040.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/usb_descriptors_rp2040.c
    )
else ()
  target_sources(${PROJECT_NAME} PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/src/link_stm32f4.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/plat_stm32f4.c
    )
endif ()

# src/ for tusb_config.h; firmware/src for the wire format shared with the nRF side
target_include_directories(${PROJECT_NAME} PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/src
  ${CMAKE_CURRENT_SOURCE_DIR}/../firmware/src
  )

if (FAMILY STREQUAL "rp2040")
  # USB host on the PIO port, USB device (log) on the native port
  family_configure_dual_usb_example(${PROJECT_NAME} noos)
  # the UART pads are the links; the log goes over USB instead
  pico_enable_stdio_uart(${PROJECT_NAME} 0)
  target_link_libraries(${PROJECT_NAME} PUBLIC hardware_uart hardware_watchdog)
  # Pico-PIO-USB doesn't build cleanly under TinyUSB's -Werror warning set
  target_compile_options(${PROJECT_NAME} PUBLIC
    -Wno-error=shadow
    -Wno-error=cast-align
    -Wno-error=cast-qual
    -Wno-error=redundant-decls
    -Wno-error=sign-conversion
    -Wno-error=conversion
    -Wno-error=sign-compare
    -Wno-error=unused-function
    )
else ()
  family_configure_host_example(${PROJECT_NAME} noos)
endif ()
