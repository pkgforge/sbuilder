pub const ELF_MAGIC_BYTES: [u8; 4] = [0x7f, 0x45, 0x4c, 0x46];
pub const APPIMAGE_MAGIC_BYTES: [u8; 4] = [0x41, 0x49, 0x02, 0x00];
pub const FLATIMAGE_MAGIC_BYTES: [u8; 4] = [0x46, 0x49, 0x01, 0x00];

// onelf packs a directory into a self-extracting ELF whose last 76 bytes are a
// fixed footer. The footer starts with "ONELF\0\x01\x00"; the file itself ends
// with "FLENONE\0". Since the file begins with ELF magic, onelf can only be told
// apart from a plain static ELF by reading this trailing footer magic.
pub const ONELF_MAGIC_BYTES: [u8; 8] = [0x4f, 0x4e, 0x45, 0x4c, 0x46, 0x00, 0x01, 0x00];
pub const ONELF_FOOTER_SIZE: u64 = 76;

pub const PNG_MAGIC_BYTES: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
pub const SVG_MAGIC_BYTES: [u8; 4] = [0x3c, 0x73, 0x76, 0x67];
pub const XML_MAGIC_BYTES: [u8; 5] = [0x3c, 0x3f, 0x78, 0x6d, 0x6c];

pub const MIN_ICON_SIZE: u64 = 20;
pub const MIN_DESKTOP_SIZE: u64 = 8;
