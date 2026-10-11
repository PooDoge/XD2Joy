//! Firmware updates through the UF2 bootloader: checking an image before it goes anywhere, finding
//! the drive a board shows while in its bootloader, and copying the image there.
//!
//! Files: `lib.rs` (errors, the nRF52840 family id), `uf2.rs` (image checks), `drive.rs` (finding
//! bootloader drives, waiting for a new one, copying).

mod drive;
mod uf2;

use std::path::PathBuf;

pub use drive::{BootDrive, BootInfo, check_layout, copy_image, list_boot_drives, new_drive, scan_mounts};
pub use uf2::{Uf2Error, Uf2Image};

/// UF2 family id of the nRF52840 (Adafruit's assignment, which the Zephyr `uf2` boards use).
pub const FAMILY_NRF52840: u32 = 0xADA5_2840;

#[derive(Debug, thiserror::Error)]
pub enum FlashError {
    #[error(transparent)]
    Image(#[from] Uf2Error),
    #[error("could not read {path}: {source}")]
    ReadImage {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write the firmware to {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "this firmware starts at {found:#x}, but {model} (SoftDevice {softdevice}) runs firmware from {board:#x}, \
         so it was built for another kind of board"
    )]
    WrongLayout { found: u32, board: u32, softdevice: String, model: String },
}
