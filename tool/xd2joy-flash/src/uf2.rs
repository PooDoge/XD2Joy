//! UF2 image checks. A UF2 file is a run of 512-byte blocks, each carrying up to 476 bytes of
//! payload and its flash address (format: github.com/microsoft/uf2). The bootloader writes blocks
//! as they arrive, so a broken or wrong-chip file has to be refused before the copy, not after.

const BLOCK: usize = 512;
const MAGIC_START0: u32 = 0x0A32_4655;
const MAGIC_START1: u32 = 0x9E5D_5157;
const MAGIC_END: u32 = 0x0AB1_6F30;
const FLAG_NOT_MAIN_FLASH: u32 = 0x0000_0001;
const FLAG_FAMILY_ID: u32 = 0x0000_2000;
const MAX_PAYLOAD: u32 = 476;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Uf2Error {
    #[error("the file is empty")]
    Empty,
    #[error("the file is {0} bytes, not a whole number of 512-byte UF2 blocks")]
    Size(usize),
    #[error("block {0} is not a UF2 block (bad magic numbers)")]
    Magic(usize),
    #[error("block {index} says it is block {says} of {total}")]
    Numbering { index: usize, says: u32, total: u32 },
    #[error("block {0} carries more than 476 bytes")]
    Payload(usize),
    #[error("block {0} has no chip family id, so the tool cannot tell which board it is for")]
    NoFamily(usize),
    #[error("the file mixes chip families ({0:#010x} and {1:#010x})")]
    MixedFamilies(u32, u32),
    #[error("the file is for chip family {found:#010x}, not this board's {expected:#010x}")]
    WrongFamily { expected: u32, found: u32 },
}

/// A UF2 file that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uf2Image {
    bytes: Vec<u8>,
    family: u32,
    blocks: u32,
    /// Lowest and one-past-highest flash address written.
    span: (u32, u32),
}

impl Uf2Image {
    /// Check every block: magic numbers, numbering, payload size, and one family id throughout.
    pub fn parse(bytes: Vec<u8>) -> Result<Uf2Image, Uf2Error> {
        if bytes.is_empty() {
            return Err(Uf2Error::Empty);
        }
        if !bytes.len().is_multiple_of(BLOCK) {
            return Err(Uf2Error::Size(bytes.len()));
        }
        let count = bytes.len() / BLOCK;
        let mut family = None;
        let mut span = (u32::MAX, 0u32);
        for (index, block) in bytes.chunks_exact(BLOCK).enumerate() {
            let word = |at: usize| u32::from_le_bytes([block[at], block[at + 1], block[at + 2], block[at + 3]]);
            if word(0) != MAGIC_START0 || word(4) != MAGIC_START1 || word(BLOCK - 4) != MAGIC_END {
                return Err(Uf2Error::Magic(index));
            }
            let (flags, addr, size, number, total, fam) = (word(8), word(12), word(16), word(20), word(24), word(28));
            if number as usize != index || total as usize != count {
                return Err(Uf2Error::Numbering { index, says: number, total });
            }
            if size > MAX_PAYLOAD {
                return Err(Uf2Error::Payload(index));
            }
            if flags & FLAG_FAMILY_ID == 0 {
                return Err(Uf2Error::NoFamily(index));
            }
            match family {
                None => family = Some(fam),
                Some(f) if f != fam => return Err(Uf2Error::MixedFamilies(f, fam)),
                Some(_) => {}
            }
            if flags & FLAG_NOT_MAIN_FLASH == 0 {
                span = (span.0.min(addr), span.1.max(addr.saturating_add(size)));
            }
        }
        // a non-empty file always sets the family on its first block
        let family = family.ok_or(Uf2Error::Empty)?;
        if span.0 > span.1 {
            span = (0, 0);
        }
        Ok(Uf2Image { bytes, family, blocks: count as u32, span })
    }

    /// Refuse an image built for another chip.
    pub fn require_family(&self, expected: u32) -> Result<(), Uf2Error> {
        if self.family == expected { Ok(()) } else { Err(Uf2Error::WrongFamily { expected, found: self.family }) }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn family(&self) -> u32 {
        self.family
    }

    pub fn blocks(&self) -> u32 {
        self.blocks
    }

    pub fn span(&self) -> (u32, u32) {
        self.span
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::FAMILY_NRF52840;

    /// A valid image of `n` blocks at `base`, 256 bytes each, as Zephyr's uf2 builds write them.
    pub(crate) fn image(n: u32, base: u32, family: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..n {
            let mut b = vec![0u8; BLOCK];
            let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
            put(0, MAGIC_START0);
            put(4, MAGIC_START1);
            put(8, FLAG_FAMILY_ID);
            put(12, base + i * 256);
            put(16, 256);
            put(20, i);
            put(24, n);
            put(28, family);
            put(BLOCK - 4, MAGIC_END);
            out.extend(b);
        }
        out
    }

    #[test]
    fn accepts_a_good_image() {
        let img = Uf2Image::parse(image(3, 0x1000, FAMILY_NRF52840)).expect("valid");
        assert_eq!(img.blocks(), 3);
        assert_eq!(img.span(), (0x1000, 0x1300));
        assert_eq!(img.require_family(FAMILY_NRF52840), Ok(()));
        assert!(matches!(img.require_family(0xE48B_FF56), Err(Uf2Error::WrongFamily { .. })));
    }

    #[test]
    fn refuses_broken_files() {
        assert_eq!(Uf2Image::parse(vec![]), Err(Uf2Error::Empty));
        assert_eq!(Uf2Image::parse(vec![0; 100]), Err(Uf2Error::Size(100)));
        assert_eq!(Uf2Image::parse(vec![0; 512]), Err(Uf2Error::Magic(0)));

        // a truncated download: block numbers say 3, the file holds 2
        let mut short = image(3, 0, FAMILY_NRF52840);
        short.truncate(2 * BLOCK);
        assert!(matches!(Uf2Image::parse(short), Err(Uf2Error::Numbering { index: 0, says: 0, total: 3 })));

        let mut mixed = image(1, 0, FAMILY_NRF52840);
        mixed.extend(image(1, 0, 0xE48B_FF56));
        assert!(Uf2Image::parse(mixed).is_err());

        let mut no_family = image(1, 0, FAMILY_NRF52840);
        no_family[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(Uf2Image::parse(no_family), Err(Uf2Error::NoFamily(0)));

        let mut big = image(1, 0, FAMILY_NRF52840);
        big[16..20].copy_from_slice(&477u32.to_le_bytes());
        assert_eq!(Uf2Image::parse(big), Err(Uf2Error::Payload(0)));
    }
}
