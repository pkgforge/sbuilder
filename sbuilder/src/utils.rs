use std::{
    env,
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

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

pub fn temp_file(app_id: &str, script: &str) -> PathBuf {
    let tmp_dir = env::temp_dir();
    let tmp_file_path = tmp_dir.join(format!("sbuild-{}", app_id));
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
    file.read_exact(&mut magic_bytes).unwrap();
    magic_bytes
}

pub fn calc_checksum<P: AsRef<Path>>(file_path: P) -> String {
    let mut file = File::open(&file_path).unwrap();
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 8192];

    while let Ok(n) = file.read(&mut buffer) {
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    file.flush().unwrap();
    hasher.finalize().to_string()
}
