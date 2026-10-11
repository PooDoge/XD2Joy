/* The crash record from main.c goes first in .noinit, at the low end of RAM. The Adafruit UF2
 * bootloader runs on every reset and zeroes its own RAM from 0x20008000 up, and writes its
 * double-reset flag at 0x20007F7C, so only a record below that survives into the next boot.
 * build.sh checks the address. */
KEEP(*(.noinit_jc_crash))
