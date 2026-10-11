cmake_minimum_required(VERSION 3.20.0)

if(JC_LEFT)
  list(APPEND EXTRA_DTC_OVERLAY_FILE "${CMAKE_CURRENT_LIST_DIR}/left.overlay")
  list(APPEND EXTRA_CONF_FILE "${CMAKE_CURRENT_LIST_DIR}/left.conf")
endif()
list(APPEND EXTRA_DTC_OVERLAY_FILE "${CMAKE_CURRENT_LIST_DIR}/clock_rc.overlay")
list(APPEND EXTRA_CONF_FILE "${CMAKE_CURRENT_LIST_DIR}/clock_rc.conf")
if(NOT JC_LEFT)
  list(APPEND EXTRA_DTC_OVERLAY_FILE "${CMAKE_CURRENT_LIST_DIR}/right.overlay")
  list(APPEND EXTRA_CONF_FILE "${CMAKE_CURRENT_LIST_DIR}/usb_name_right.conf")
endif()
list(APPEND EXTRA_CONF_FILE "${CMAKE_CURRENT_LIST_DIR}/tx_power.conf")
# Seeed XIAO nRF52840 / Sense (-b xiao_ble): its own pins, applied last so they win
if(BOARD MATCHES "^xiao_ble")
  list(APPEND EXTRA_DTC_OVERLAY_FILE "${CMAKE_CURRENT_LIST_DIR}/xiao.overlay")
endif()

if(NOT DEFINED JC_OUT)
  set(JC_OUT "${CMAKE_CURRENT_LIST_DIR}/out")
endif()
if(JC_OUT)
  if(JC_LEFT)
    set(jc_uf2 "${JC_OUT}/xd2joy_LEFT.uf2")
  else()
    set(jc_uf2 "${JC_OUT}/xd2joy_RIGHT.uf2")
  endif()
  set_property(GLOBAL APPEND PROPERTY extra_post_build_commands
    COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_BINARY_DIR}/zephyr/zephyr.uf2 ${jc_uf2}
    COMMAND ${CMAKE_COMMAND} -E echo Firmware copied to ${jc_uf2})
endif()

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(xd2joy)

file(READ "${ZEPHYR_BASE}/include/zephyr/bluetooth/hci_types.h" jc_hci)
string(REGEX MATCH "#define[ \t]+BT_HCI_LE_INTERVAL_MIN[ \t]+0x0004" jc_floor "${jc_hci}")
if(NOT jc_floor)
  message(FATAL_ERROR "Zephyr patch 1 is missing: BT_HCI_LE_INTERVAL_MIN in "
                      "include/zephyr/bluetooth/hci_types.h is not 0x0004 -- see "
                      "ZEPHYR_PATCHES.md at the top of this repository")
endif()

foreach(jc_patched
    subsys/bluetooth/controller/ll_sw/nordic/lll/lll_scan.c
    subsys/bluetooth/controller/ll_sw/ull_llcp_conn_upd.c)
  file(READ "${ZEPHYR_BASE}/${jc_patched}" jc_src)
  string(FIND "${jc_src}" "conn window patch" jc_at)
  if(jc_at EQUAL -1)
    message(FATAL_ERROR "Zephyr patch 2 is missing from ${jc_patched} -- see "
                        "ZEPHYR_PATCHES.md at the top of this repository")
  endif()
endforeach()
zephyr_compile_definitions(JC_CONN_WIN_OFFSET=4 JC_CONN_WIN_SIZE=3)

# reported by the `ver` console command; the desktop tool shows it and compares
# it with the firmware it bundles
execute_process(
  COMMAND git describe --tags --always --dirty
  WORKING_DIRECTORY ${CMAKE_CURRENT_LIST_DIR}
  OUTPUT_VARIABLE jc_version
  OUTPUT_STRIP_TRAILING_WHITESPACE
  ERROR_QUIET
  RESULT_VARIABLE jc_version_rc)
if(NOT jc_version_rc EQUAL 0 OR NOT jc_version)
  set(jc_version "dev")
endif()
zephyr_compile_definitions(XD2JOY_FW_VERSION="${jc_version}")

target_sources(app PRIVATE
    src/chip_addr.c
    src/console_keep.c
    src/main.c
    src/bridge.c
    src/hci_user_ext.c)
target_include_directories(app PRIVATE
    ${ZEPHYR_BASE}/subsys/bluetooth/controller
    ${ZEPHYR_BASE}/subsys/bluetooth/controller/ll_sw/nordic)
target_include_directories(app PRIVATE ${ZEPHYR_BASE}/subsys/bluetooth)
zephyr_include_directories(src)

if(JC_LEFT)
  target_compile_definitions(app PRIVATE JC_LEFT=1)
  target_sources(app PRIVATE src/usb_dev.c)
endif()
