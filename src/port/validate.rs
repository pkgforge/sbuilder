//! Structural validation of a ports tree, intended as a CI gate.
//!
//! The load-bearing rule is that every pinned URL has a hash beside it in the
//! same file. A version file without one is worse than no file at all: it
//! looks pinned while trusting whatever the server happens to return.

use std::path::Path;

use crate::port::tree;

pub struct Report {
    pub checked: usize,
    pub packages: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn run(root: &Path) -> Report {
    let (packages, mut errors) = tree::load(root);
    let mut warnings = Vec::new();
    let mut checked = 0usize;

    for p in &packages {
        let name = p.dir.file_name().unwrap_or_default().to_string_lossy().to_string();

        if p.pkg.pkg.name.is_empty() {
            errors.push(format!("{name}: pkg.name missing"));
        }
        if p.pkg.pkg.description.as_deref().unwrap_or("").is_empty() {
            errors.push(format!("{name}: pkg.description missing"));
        }

        if p.pkg.pkg.disabled {
            if p.pkg.pkg.disabled_reason.is_none() {
                warnings.push(format!("{name}: disabled without a reason"));
            }
            continue;
        }

        if p.pkg.host.supported.is_empty() {
            errors.push(format!("{name}: no supported hosts"));
        }
        if p.versions.is_empty() {
            errors.push(format!("{name}: no version file"));
            continue;
        }

        for v in &p.versions {
            let tag = format!("{name}-{}", v.version);
            if v.version.is_empty() {
                errors.push(format!("{name}: version file with no version"));
            }
            if v.url.is_empty() {
                errors.push(format!("{tag}: no url"));
            }
            for (host, url) in &v.url {
                checked += 1;
                if url.contains("${") {
                    errors.push(format!("{tag}: unexpanded template in {host} url"));
                }
                let b3 = v.blake3.get(host);
                let sha = v.sha256.get(host);
                if b3.is_none() && sha.is_none() {
                    errors.push(format!("{tag}: {host} pinned with NO HASH"));
                } else if b3.is_none() {
                    // soar's index carries a bsum column only, so an entry
                    // without blake3 reaches the client unverifiable.
                    warnings.push(format!("{tag}: {host} has no blake3; soar cannot verify it"));
                }
            }
            for host in v.blake3.keys().chain(v.sha256.keys()) {
                if !v.url.contains_key(host) {
                    errors.push(format!("{tag}: hash for {host} with no url"));
                }
            }
        }
    }

    Report { checked, packages: packages.len(), errors, warnings }
}
