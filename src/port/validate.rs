//! Structural validation of a ports tree, intended as a CI gate.
//!
//! The load-bearing rule is that every pinned URL has a hash beside it in the
//! same file. A version file without one is worse than no file at all: it
//! looks pinned while trusting whatever the server happens to return.

use std::collections::HashSet;
use std::{fs, path::Path, path::PathBuf};

use crate::port::model::VersionToml;
use crate::port::tree;

pub struct Report {
    pub checked: usize,
    pub packages: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn run(root: &Path, files: &[PathBuf], strict: bool) -> Report {
    let (packages, mut errors) = tree::load(root);
    let mut warnings = Vec::new();
    let mut checked = 0usize;

    // Naming version files scopes the run to those pins, so the update flow
    // can judge a new pin without stale gaps in old pins voting on it. Strict
    // mode is what a merge has to guarantee: every host the package claims is
    // pinned, and every pinned host carries a blake3 digest. Files are
    // matched by name rather than full path: the caller's prefix (`./`,
    // absolute, relative) need not agree with the tree walk's.
    let scoped = !files.is_empty();
    let wanted: HashSet<String> = files
        .iter()
        .filter_map(|f| f.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();

    for p in &packages {
        let name = p.dir.file_name().unwrap_or_default().to_string_lossy().to_string();

        // A scoped run judges pins, not recipes: the package-level rules are
        // the whole-tree run's job, and a disabled package has nothing a
        // client can install.
        if scoped {
            if p.pkg.pkg.disabled {
                continue;
            }
            for (v, path) in p.versions.iter().zip(version_files(&p.dir)) {
                let known = path.file_name().is_some_and(|n| wanted.contains(&n.to_string_lossy().to_string()));
                if !known {
                    continue;
                }
                check_pin(p, v, strict, &mut errors, &mut warnings, &mut checked);
            }
            continue;
        }

        if p.pkg.pkg.name.is_empty() {
            errors.push(format!("{name}: pkg.name missing"));
        }
        if p.pkg.pkg.description.as_deref().unwrap_or("").is_empty() {
            errors.push(format!("{name}: pkg.description missing"));
        }

        // An install target names a path inside the package directory, so it
        // must stay inside it, and the prefix decides where soar links the
        // file: a typo in `share/man` silently installs somewhere nothing reads.
        if let Some(src) = &p.pkg.source {
            for entry in &src.install.all_entries() {
                let to = &entry.target();
                if to.starts_with('/') || to.starts_with('~') {
                    errors.push(format!("{name}: install target {to:?} is not relative"));
                    continue;
                }
                if to.split('/').any(|c| c == ".." || c == ".") {
                    errors.push(format!("{name}: install target {to:?} escapes the package"));
                    continue;
                }
                if let Some((prefix, _)) = to.split_once('/') {
                    const KNOWN: [&str; 2] = ["bin", "share"];
                    if !KNOWN.contains(&prefix) {
                        warnings.push(format!(
                            "{name}: install target {to:?} starts with {prefix:?}, \
                             which soar does not link anywhere"
                        ));
                    }
                }
            }
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
            check_pin(p, v, strict, &mut errors, &mut warnings, &mut checked);
        }
    }

    Report { checked, packages: packages.len(), errors, warnings }
}

/// The version files of one package, sorted, which is the order
/// `p.versions` was loaded in.
fn version_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|q| {
            q.extension().is_some_and(|x| x == "toml")
                && q.file_name().is_some_and(|f| f != "pkg.toml")
        })
        .collect();
    paths.sort();
    paths
}

fn check_pin(
    p: &tree::Package,
    v: &VersionToml,
    strict: bool,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
    checked: &mut usize,
) {
    let name = p.dir.file_name().unwrap_or_default().to_string_lossy().to_string();
    let tag = format!("{name}-{}", v.version);
    if v.version.is_empty() {
        errors.push(format!("{name}: version file with no version"));
    }
    if v.url.is_empty() {
        errors.push(format!("{tag}: no url"));
    }
    for (host, url) in &v.url {
        *checked += 1;
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
            let msg = format!("{tag}: {host} has no blake3; soar cannot verify it");
            if strict {
                errors.push(msg);
            } else {
                warnings.push(msg);
            }
        }
    }
    for host in v.blake3.keys().chain(v.sha256.keys()) {
        if !v.url.contains_key(host) {
            errors.push(format!("{tag}: hash for {host} with no url"));
        }
    }
    // A release can lag the recipe: the host is claimed but upstream never
    // published the artifact for this version, and the pin would be
    // uninstallable there.
    if strict {
        for host in &p.pkg.host.supported {
            if !v.url.contains_key(host) {
                errors.push(format!("{tag}: {host} is supported but not pinned"));
            }
        }
    }

    // One URL serving several hosts is only right when the artifact
    // holds every architecture and the install map picks between them.
    // Without that, one architecture is being handed another's binary.
    let distinct: std::collections::BTreeSet<&String> = v.url.values().collect();
    if v.url.len() > 1 && distinct.len() == 1 {
        let selects_arch = p
            .pkg
            .source
            .as_ref()
            .is_some_and(|s| {
                s.install.all_entries()
                    .iter()
                    .any(|e| e.from.as_deref().is_some_and(|f| f.contains("${arch}")))
            });
        if !selects_arch {
            errors.push(format!(
                "{tag}: one url for {} hosts and no ${{arch}} in the install map",
                v.url.len()
            ));
        }
    }
}
