//! Reading icons and desktop files out of onelf-packed binaries.
//!
//! An onelf file is a self-extracting ELF with a trailing 76-byte footer
//! pointing at a zstd-compressed manifest and a block-compressed payload. This
//! module reads just enough of that format (via the published `onelf-format`
//! type definitions) to pull out the bundled icon and `.desktop` file, which
//! live under the `.onelf/` metadata convention:
//!
//! - icons:   `.onelf/icons/{entrypoint}.svg`, `{entrypoint}.png`,
//!            `default.svg`, `default.png`
//! - desktop: `.onelf/desktop/{entrypoint}.desktop`, `default.desktop`
//!
//! See the onelf file-format reference for the on-disk layout.

use std::{
    fs::{self, File},
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::Path,
};

use onelf_format::{Entry, EntryKind, Footer, Manifest, FOOTER_SIZE};

/// A parsed onelf package, holding its footer and manifest plus the path to
/// the backing file so individual entries can be decompressed on demand.
pub struct OnelfPackage {
    file: File,
    footer: Footer,
    manifest: Manifest,
    dict: Option<Vec<u8>>,
}

impl OnelfPackage {
    /// Parse the footer and manifest of an onelf-packed binary.
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let file_size = file.metadata()?.len();
        if file_size < FOOTER_SIZE as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file too small for onelf footer",
            ));
        }

        file.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;
        let mut footer_buf = [0u8; FOOTER_SIZE];
        file.read_exact(&mut footer_buf)?;
        let footer = Footer::from_bytes(&footer_buf)?;

        file.seek(SeekFrom::Start(footer.manifest_offset))?;
        let mut compressed = vec![0u8; footer.manifest_compressed as usize];
        file.read_exact(&mut compressed)?;
        let manifest_bytes = zstd::bulk::decompress(&compressed, footer.manifest_original as usize)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("manifest decompression failed: {e}"),
                )
            })?;
        let manifest = Manifest::deserialize(&manifest_bytes)?;

        let dict = if footer.dict_size > 0 {
            file.seek(SeekFrom::Start(footer.dict_offset))?;
            let mut buf = vec![0u8; footer.dict_size as usize];
            file.read_exact(&mut buf)?;
            Some(buf)
        } else {
            None
        };

        Ok(Self {
            file,
            footer,
            manifest,
            dict,
        })
    }

    /// Find the bundled icon entry for any of the given entrypoint names,
    /// falling back to the package defaults. Returns the entry index and the
    /// file extension implied by the resolved path (`svg` or `png`).
    fn resolve_icon(&self, entrypoints: &[&str]) -> Option<(usize, &'static str)> {
        for ep in entrypoints {
            if let Some(idx) = self.find_file(&format!(".onelf/icons/{ep}.svg")) {
                return Some((idx, "svg"));
            }
            if let Some(idx) = self.find_file(&format!(".onelf/icons/{ep}.png")) {
                return Some((idx, "png"));
            }
        }
        if let Some(idx) = self.find_file(".onelf/icons/default.svg") {
            return Some((idx, "svg"));
        }
        if let Some(idx) = self.find_file(".onelf/icons/default.png") {
            return Some((idx, "png"));
        }
        None
    }

    /// Find the bundled `.desktop` entry for any of the given entrypoint names,
    /// falling back to the package default.
    fn resolve_desktop(&self, entrypoints: &[&str]) -> Option<usize> {
        for ep in entrypoints {
            if let Some(idx) = self.find_file(&format!(".onelf/desktop/{ep}.desktop")) {
                return Some(idx);
            }
        }
        self.find_file(".onelf/desktop/default.desktop")
    }

    /// Name of the package's default entrypoint, if any.
    fn default_entrypoint(&self) -> Option<&str> {
        let idx = self.manifest.header.default_entrypoint as usize;
        self.manifest
            .entrypoints
            .get(idx)
            .map(|ep| self.manifest.get_string(ep.name))
    }

    fn find_file(&self, path: &str) -> Option<usize> {
        self.manifest
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == EntryKind::File)
            .find(|(i, _)| self.manifest.entry_path(*i) == path)
            .map(|(i, _)| i)
    }

    /// Decompress and concatenate an entry's payload blocks into its full
    /// file content.
    fn read_entry(&mut self, entry: &Entry) -> io::Result<Vec<u8>> {
        let mut result = Vec::new();
        for block in &entry.blocks {
            self.file.seek(SeekFrom::Start(
                self.footer.payload_offset + block.payload_offset,
            ))?;
            let mut compressed = vec![0u8; block.compressed_size as usize];
            self.file.read_exact(&mut compressed)?;

            // Stored mode: payload bytes are the file content verbatim.
            if self.footer.is_stored() {
                result.extend_from_slice(&compressed);
                continue;
            }

            let decompressed = if let Some(dict) = &self.dict {
                let mut decoder = zstd::Decoder::with_dictionary(Cursor::new(&compressed), dict)?;
                let mut buf = Vec::with_capacity(block.original_size as usize);
                decoder.read_to_end(&mut buf)?;
                buf
            } else {
                zstd::bulk::decompress(&compressed, block.original_size as usize).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("block decompression failed: {e}"),
                    )
                })?
            };
            result.extend_from_slice(&decompressed);
        }
        Ok(result)
    }

    /// Extract the bundled icon to `dest`, matching the icon to `cmd` (or the
    /// package default). Returns the destination path on success, or `None`
    /// when the package ships no icon.
    pub fn extract_icon<P: AsRef<Path>>(&mut self, cmd: &str, dest: P) -> io::Result<Option<()>> {
        let default_ep = self.default_entrypoint().map(str::to_owned);
        let mut names = vec![cmd];
        if let Some(ep) = &default_ep {
            if ep != cmd {
                names.push(ep);
            }
        }
        let Some((idx, _ext)) = self.resolve_icon(&names) else {
            return Ok(None);
        };
        let entry = self.manifest.entries[idx].clone();
        let data = self.read_entry(&entry)?;
        fs::write(dest, data)?;
        Ok(Some(()))
    }

    /// Extract the bundled `.desktop` file to `dest`, matching `cmd` (or the
    /// package default). Returns the destination path on success, or `None`
    /// when the package ships no desktop file.
    pub fn extract_desktop<P: AsRef<Path>>(
        &mut self,
        cmd: &str,
        dest: P,
    ) -> io::Result<Option<()>> {
        let default_ep = self.default_entrypoint().map(str::to_owned);
        let mut names = vec![cmd];
        if let Some(ep) = &default_ep {
            if ep != cmd {
                names.push(ep);
            }
        }
        let Some(idx) = self.resolve_desktop(&names) else {
            return Ok(None);
        };
        let entry = self.manifest.entries[idx].clone();
        let data = self.read_entry(&entry)?;
        fs::write(dest, data)?;
        Ok(Some(()))
    }
}
