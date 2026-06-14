//! End-to-end extraction test against a real onelf package.
//!
//! Ignored by default; run with the path to a packed `.onelf` file:
//!
//! ```sh
//! ONELF_TEST_FILE=/path/to/myapp.onelf cargo test -p sbuild --test onelf_extract -- --ignored --nocapture
//! ```

use std::env;
use std::path::Path;

use sbuild::onelf::OnelfPackage;

#[test]
#[ignore]
fn extracts_icon_and_desktop() {
    let path = env::var("ONELF_TEST_FILE").expect("set ONELF_TEST_FILE");
    let cmd = env::var("ONELF_TEST_CMD").unwrap_or_else(|_| "myapp".to_string());
    let dir = tempfile::tempdir().unwrap();

    let mut pkg = OnelfPackage::open(&path).expect("open onelf");

    let icon_dest = dir.path().join("out.icon");
    let got = pkg.extract_icon(&cmd, &icon_dest).expect("extract icon");
    assert!(got.is_some(), "expected an icon to be extracted");
    let icon = std::fs::read(&icon_dest).unwrap();
    assert!(!icon.is_empty(), "icon should not be empty");
    eprintln!("icon: {} bytes", icon.len());

    let desktop_dest = dir.path().join("out.desktop");
    let got = pkg
        .extract_desktop(&cmd, &desktop_dest)
        .expect("extract desktop");
    assert!(got.is_some(), "expected a desktop file to be extracted");
    let desktop = std::fs::read_to_string(&desktop_dest).unwrap();
    assert!(
        desktop.contains("[Desktop Entry]"),
        "desktop file should be a real .desktop, got: {desktop}"
    );
    eprintln!("desktop:\n{desktop}");
}

/// Mirrors the builder's dest-dir computation: for a sub-package binary at
/// `packages/<parent>/<cmd>`, extracted assets must land in that same dir,
/// not the root.
#[test]
#[ignore]
fn extracts_next_to_subpackage_binary() {
    let path = env::var("ONELF_TEST_FILE").expect("set ONELF_TEST_FILE");
    let cmd = env::var("ONELF_TEST_CMD").unwrap_or_else(|_| "amdgpu_top".to_string());
    let provide_path = Path::new(&path);
    let dest_dir = provide_path.parent().unwrap_or_else(|| Path::new(""));

    let mut pkg = OnelfPackage::open(provide_path).expect("open onelf");

    let desktop_dest = dest_dir.join(format!("{cmd}.desktop"));
    let got = pkg
        .extract_desktop(&cmd, &desktop_dest)
        .expect("extract desktop");
    assert!(got.is_some(), "expected a desktop file");
    assert!(
        desktop_dest.exists(),
        "desktop should be written beside the binary at {}",
        desktop_dest.display()
    );
    eprintln!("desktop written to: {}", desktop_dest.display());
}
