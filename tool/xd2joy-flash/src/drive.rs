//! Bootloader drives. In its bootloader a board shows up as a small USB drive (`NICENANO` on Pro
//! Micro style boards, `XIAO-SENSE` on a Seeed XIAO) holding `INFO_UF2.TXT`; copying a `.uf2`
//! file onto it flashes the board. Both halves show the same name, so the tool tells them apart
//! by which drive is *new* after it sent one board the `bootloader` command.

use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use crate::{FlashError, Uf2Image};

const INFO_FILE: &str = "INFO_UF2.TXT";
/// The bootloader ignores the name; this one makes it obvious in a file manager what happened.
const TARGET_NAME: &str = "XD2JOY.UF2";

/// What `INFO_UF2.TXT` says about the bootloader.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootInfo {
    /// First line, e.g. `UF2 Bootloader 0.6.0 lib/nrfx ...`.
    pub bootloader: String,
    pub model: Option<String>,
    pub board_id: Option<String>,
    /// e.g. `S140 version 7.3.0`.
    pub softdevice: Option<String>,
}

impl BootInfo {
    pub fn parse(text: &str) -> BootInfo {
        let mut lines = text.lines();
        let bootloader = lines.next().unwrap_or_default().trim().to_owned();
        let mut info = BootInfo { bootloader, ..BootInfo::default() };
        for line in lines {
            if let Some((key, value)) = line.split_once(':') {
                let value = value.trim().to_owned();
                match key.trim() {
                    "Model" => info.model = Some(value),
                    "Board-ID" => info.board_id = Some(value),
                    "SoftDevice" => info.softdevice = Some(value),
                    _ => {}
                }
            }
        }
        info
    }

    /// Whether this is an nRF52840 bootloader. Unknown boards are not refused (clones write all
    /// sorts of ids), but the caller can warn.
    pub fn looks_like_nrf52840(&self) -> bool {
        self.board_id.as_deref().is_some_and(|id| id.contains("nRF52840"))
    }

    /// Where this bootloader starts firmware: right after the SoftDevice it keeps, which is S140
    /// 6.x on nice!nano and Pro Micro clones (0x26000) and 7.x on the Seeed XIAO (0x27000). `None`
    /// for anything else, which the caller does not check.
    pub fn app_start(&self) -> Option<u32> {
        let version = self.softdevice.as_deref()?.strip_prefix("S140 version ")?;
        match version.split('.').next()? {
            "6" => Some(0x26000),
            "7" => Some(0x27000),
            _ => None,
        }
    }
}

/// Refuse an image that would not start on this board: firmware linked for one SoftDevice layout
/// copied onto a bootloader with the other either overwrites the end of the SoftDevice or is never
/// jumped to. Both leave the board without working firmware until it is flashed again.
pub fn check_layout(info: &BootInfo, image: &Uf2Image) -> Result<(), FlashError> {
    let Some(board) = info.app_start() else { return Ok(()) };
    let found = image.span().0;
    if found == board {
        Ok(())
    } else {
        Err(FlashError::WrongLayout {
            found,
            board,
            softdevice: info.softdevice.clone().unwrap_or_default(),
            model: info.model.clone().unwrap_or_else(|| "this board".to_owned()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootDrive {
    pub mount: PathBuf,
    pub info: BootInfo,
}

/// Bootloader drives among the mounted file systems.
pub fn list_boot_drives() -> Vec<BootDrive> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    scan_mounts(disks.list().iter().map(|d| d.mount_point().to_path_buf()))
}

/// Bootloader drives among `mounts` (split out from [`list_boot_drives`] for tests).
pub fn scan_mounts(mounts: impl IntoIterator<Item = PathBuf>) -> Vec<BootDrive> {
    let mut drives: Vec<BootDrive> = mounts
        .into_iter()
        .filter_map(|mount| {
            let text = std::fs::read_to_string(mount.join(INFO_FILE)).ok()?;
            Some(BootDrive { info: BootInfo::parse(&text), mount })
        })
        .collect();
    drives.sort_by(|a, b| a.mount.cmp(&b.mount));
    drives
}

/// The one drive in `now` that was not in `before`. `None` when there is none yet, or when more
/// than one appeared (the tool cannot know which board is which, so it does not guess).
pub fn new_drive(before: &[BootDrive], now: &[BootDrive]) -> Option<BootDrive> {
    let mut fresh = now.iter().filter(|d| !before.iter().any(|b| b.mount == d.mount));
    match (fresh.next(), fresh.next()) {
        (Some(one), None) => Some(one.clone()),
        _ => None,
    }
}

/// Write the image to the drive. The board restarts as soon as it has the last block, and some
/// systems then report an error closing or syncing a file on a drive that is gone; once every byte
/// was handed over, that error is logged and the copy counts as done.
pub fn copy_image(drive: &Path, image: &Uf2Image) -> Result<(), FlashError> {
    let path = drive.join(TARGET_NAME);
    let write_err = |source| FlashError::Write { path: path.clone(), source };
    let mut file = File::create(&path).map_err(write_err)?;
    file.write_all(image.bytes()).map_err(write_err)?;
    if let Err(e) = file.flush().and_then(|()| file.sync_all()) {
        if drive.join(INFO_FILE).exists() {
            return Err(write_err(e));
        }
        tracing::info!(path = %path.display(), error = %e, "the drive left while syncing; the board took the image");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FAMILY_NRF52840, Uf2Error, uf2::tests::image};

    const NICENANO: &str = "UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0) lib/tinyusb (0.10.1)\n\
                            Model: nice!nano\nBoard-ID: nRF52840-nicenano\nSoftDevice: S140 version 6.1.1\n";

    #[test]
    fn reads_the_info_file() {
        let info = BootInfo::parse(NICENANO);
        assert_eq!(info.bootloader, "UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0) lib/tinyusb (0.10.1)");
        assert_eq!(info.model.as_deref(), Some("nice!nano"));
        assert!(info.looks_like_nrf52840());
        assert!(
            !BootInfo::parse("UF2 Bootloader v3.0\nModel: Raspberry Pi RP2\nBoard-ID: RPI-RP2\n").looks_like_nrf52840()
        );
    }

    const XIAO: &str = "UF2 Bootloader 0.6.1 lib/nrfx (v2.0.0) lib/tinyusb (0.10.1-293-gaf8e5a90) lib/uf2 \
                        (remotes/origin/configupdate-9-gadbb8c7)\nModel: Seeed XIAO nRF52840\n\
                        Board-ID: Seeed_XIAO_nRF52840_Sense\nSoftDevice: S140 version 7.3.0\nDate: Nov 12 2021\n";

    #[test]
    fn firmware_must_start_where_the_bootloader_jumps() -> Result<(), Uf2Error> {
        let (nice, xiao) = (BootInfo::parse(NICENANO), BootInfo::parse(XIAO));
        assert!(xiao.looks_like_nrf52840());
        assert_eq!((nice.app_start(), xiao.app_start()), (Some(0x26000), Some(0x27000)));
        let pro_micro_build = Uf2Image::parse(image(2, 0x26000, FAMILY_NRF52840))?;
        let xiao_build = Uf2Image::parse(image(2, 0x27000, FAMILY_NRF52840))?;
        assert!(check_layout(&nice, &pro_micro_build).is_ok());
        assert!(check_layout(&xiao, &xiao_build).is_ok());
        assert!(matches!(
            check_layout(&xiao, &pro_micro_build),
            Err(FlashError::WrongLayout { found: 0x26000, board: 0x27000, .. })
        ));
        assert!(check_layout(&nice, &xiao_build).is_err());
        // a bootloader that names no SoftDevice is not checked
        assert!(check_layout(&BootInfo::parse("UF2 Bootloader\nBoard-ID: nRF52840-x\n"), &xiao_build).is_ok());
        Ok(())
    }

    #[test]
    fn finds_drives_and_the_new_one() -> std::io::Result<()> {
        let root = tempfile::tempdir()?;
        let (a, b, plain) = (root.path().join("a"), root.path().join("b"), root.path().join("usb-stick"));
        for d in [&a, &b, &plain] {
            std::fs::create_dir(d)?;
        }
        std::fs::write(a.join(INFO_FILE), NICENANO)?;
        let before = scan_mounts([a.clone(), b.clone(), plain.clone()]);
        assert_eq!(before.len(), 1);
        assert_eq!(new_drive(&before, &before), None);

        std::fs::write(b.join(INFO_FILE), NICENANO)?;
        let now = scan_mounts([a.clone(), b.clone(), plain]);
        assert_eq!(new_drive(&before, &now).map(|d| d.mount), Some(b));
        // two new drives at once: refuse to guess
        assert_eq!(new_drive(&[], &now), None);
        Ok(())
    }

    #[test]
    fn copies_the_image() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join(INFO_FILE), NICENANO)?;
        let img = Uf2Image::parse(image(2, 0x1000, FAMILY_NRF52840))?;
        copy_image(dir.path(), &img)?;
        assert_eq!(std::fs::read(dir.path().join(TARGET_NAME))?, img.bytes());
        Ok(())
    }
}
