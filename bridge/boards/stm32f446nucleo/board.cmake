set(MCU_VARIANT stm32f446xx)
set(JLINK_DEVICE stm32f446re)

set(LD_FILE_GNU ${CMAKE_CURRENT_LIST_DIR}/STM32F446RE_FLASH.ld)

function(update_board TARGET)
  target_compile_definitions(${TARGET} PUBLIC
    STM32F446xx
    HSE_VALUE=8000000
    )
endfunction()
