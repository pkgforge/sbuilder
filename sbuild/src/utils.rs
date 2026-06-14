use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use futures::StreamExt;
use glob::glob;
use goblin::elf::Elf;
use memmap2::Mmap;
use reqwest::header::USER_AGENT;
use sbuild_linter::logger::TaskLogger;

pub async fn download<P: AsRef<Path>>(url: &str, out: P) -> Result<(), String> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header(USER_AGENT, "pkgforge/soar")
        .send()
        .await
        .unwrap();

    if !response.status().is_success() {
        return Err(format!("Error downloading {}", url));
    }

    let output_path = out.as_ref();
    if let Some(output_dir) = output_path.parent() {
        if !output_dir.exists() {
            fs::create_dir_all(output_dir).unwrap();
        }
    }

    let temp_path = format!("{}.part", output_path.display());
    let mut stream = response.bytes_stream();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&temp_path)
        .unwrap();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap();
        file.write_all(&chunk).unwrap();
    }

    fs::rename(&temp_path, output_path).unwrap();

    Ok(())
}

pub fn extract_filename(url: &str) -> String {
    Path::new(url)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| {
            let dt = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();
            dt.to_string()
        })
}

pub fn temp_file(pkg_id: &str, script: &str) -> PathBuf {
    let tmp_dir = env::temp_dir();
    let tmp_file_path = tmp_dir.join(format!("sbuild-{}", pkg_id));
    {
        let mut tmp_file =
            File::create(&tmp_file_path).expect("Failed to create temporary script file");
        tmp_file
            .write_all(script.as_bytes())
            .expect("Failed to write to temporary script file");

        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp_file_path)
            .expect("Failed to read file metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tmp_file_path, perms).expect("Failed to set executable permissions");
    }
    tmp_file_path
}

pub fn calc_magic_bytes<P: AsRef<Path>>(file_path: P, size: usize) -> Vec<u8> {
    let file = File::open(file_path).unwrap();
    let mut file = BufReader::new(file);
    let mut magic_bytes = vec![0u8; size];
    if file.read_exact(&mut magic_bytes).is_ok() {
        file.rewind().unwrap();
        return magic_bytes;
    };
    file.rewind().unwrap();
    magic_bytes
}

pub fn pack_appimage<P: AsRef<Path>>(
    env_vars: Vec<(String, String)>,
    path: P,
    output_path: P,
    logger: &TaskLogger,
) -> bool {
    let Ok(aitool) = which::which("appimagetool") else {
        logger.warn("appimagetool not found.");
        return false;
    };

    let mut child = Command::new(aitool)
        .env_clear()
        .envs(env_vars)
        .args([
            "--comp",
            "zstd",
            "--mksquashfs-opt",
            "-root-owned",
            "--mksquashfs-opt",
            "-no-xattrs",
            "--mksquashfs-opt",
            "-noappend",
            "--mksquashfs-opt",
            "-b",
            "--mksquashfs-opt",
            "1M",
            "--mksquashfs-opt",
            "-mkfs-time",
            "--mksquashfs-opt",
            "0",
            "--mksquashfs-opt",
            "-Xcompression-level",
            "--mksquashfs-opt",
            "22",
            "--no-appstream",
            &path.as_ref().to_string_lossy().to_string(),
            &output_path.as_ref().to_string_lossy().to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .unwrap();

    let _ = child.wait().unwrap();
    true
}

pub fn self_extract_appimage(cmd: &str, mut pattern: String, dest: &str) {
    for _ in 0..10 {
        let mut child = Command::new(format!("./{}", cmd))
            .env_clear()
            .args(["--appimage-extract", pattern.as_ref()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .unwrap();

        let result = child.wait().unwrap();
        if result.success() {
            let search_pattern = format!("squashfs-root/{}", pattern);
            for entry in glob(&search_pattern).unwrap().filter_map(Result::ok) {
                fs::rename(&entry, dest).unwrap();
                break;
            }
        }

        if let Ok(link) = fs::read_link(dest) {
            pattern = link
                .to_string_lossy()
                .into_owned()
                .trim_start_matches("./")
                .to_string();
            continue;
        }

        break;
    }
}

/// Detect an onelf-packed binary by its trailing footer magic.
///
/// onelf files start with an ELF runtime stub, so they cannot be distinguished
/// from a plain static ELF by leading magic bytes. Instead, the last
/// `ONELF_FOOTER_SIZE` bytes hold a fixed footer whose first 8 bytes are
/// `ONELF_MAGIC_BYTES`.
pub fn is_onelf<P: AsRef<Path>>(file_path: P) -> bool {
    use crate::constant::{ONELF_FOOTER_SIZE, ONELF_MAGIC_BYTES};

    let Ok(mut file) = File::open(&file_path) else {
        return false;
    };
    let Ok(size) = file.metadata().map(|m| m.len()) else {
        return false;
    };
    if size < ONELF_FOOTER_SIZE {
        return false;
    }
    if file
        .seek(SeekFrom::Start(size - ONELF_FOOTER_SIZE))
        .is_err()
    {
        return false;
    }
    let mut magic = [0u8; 8];
    if file.read_exact(&mut magic).is_err() {
        return false;
    }
    magic == ONELF_MAGIC_BYTES
}

pub fn is_static_elf<P: AsRef<Path>>(file_path: P) -> bool {
    let file = File::open(&file_path).unwrap();
    let mmap = unsafe { Mmap::map(&file).unwrap() };
    let elf = Elf::parse(&mmap).unwrap();
    elf.interpreter.is_none()
}

pub fn expand_env_vars(input: &str, vars: &[(String, String)]) -> String {
    let mut result = input.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("${{{}}}", key), value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constant::{ONELF_FOOTER_SIZE, ONELF_MAGIC_BYTES};
    use tempfile::NamedTempFile;

    #[test]
    fn detects_onelf_footer_magic() {
        let mut file = NamedTempFile::new().unwrap();
        // ELF-looking prefix so leading magic alone wouldn't distinguish it.
        file.write_all(&[0x7f, 0x45, 0x4c, 0x46]).unwrap();
        // A 76-byte footer beginning with the onelf magic.
        let mut footer = vec![0u8; ONELF_FOOTER_SIZE as usize];
        footer[..ONELF_MAGIC_BYTES.len()].copy_from_slice(&ONELF_MAGIC_BYTES);
        file.write_all(&footer).unwrap();
        file.flush().unwrap();

        assert!(is_onelf(file.path()));
    }

    #[test]
    fn plain_elf_is_not_onelf() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0x7f, 0x45, 0x4c, 0x46]).unwrap();
        file.write_all(&vec![0u8; ONELF_FOOTER_SIZE as usize])
            .unwrap();
        file.flush().unwrap();

        assert!(!is_onelf(file.path()));
    }

    #[test]
    fn too_small_file_is_not_onelf() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&ONELF_MAGIC_BYTES).unwrap();
        file.flush().unwrap();

        assert!(!is_onelf(file.path()));
    }
}
