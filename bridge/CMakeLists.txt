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
  ${CMAKE_CURRENT_SOURCE_DIR}/src/link.c
  )

# src/ for tusb_config.h; firmware/src for the wire format shared with the nRF side
target_include_directories(${PROJECT_NAME} PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/src
  ${CMAKE_CURRENT_SOURCE_DIR}/../firmware/src
  )

family_configure_host_example(${PROJECT_NAME} noos)
