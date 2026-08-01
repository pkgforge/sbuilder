//! Metadata generation from a ports tree.
//!
//! This is a pure function: tree in, index out. No network, no database, no
//! build state. Running it on a clean checkout reproduces the same bytes,
//! which is what makes the published index auditable.

use std::path::Path;

use serde::Serialize;

use crate::port::{model::PkgToml, tree};


/// The index format this generator emits.
///
/// Published so a client can tell an index it cannot read from one that merely
/// lacks a field, and say which of the two it is.
pub const FORMAT: u32 = 1;

/// A generated index: the format it follows, and the packages in it.
#[derive(Debug, Serialize)]
pub struct Index {
    pub format: u32,
    pub packages: Vec<Entry>,
}

impl Index {
    pub fn new(packages: Vec<Entry>) -> Self {
        Self { format: FORMAT, packages }
    }
}

/// One index entry: a single produced binary at a single version.
#[derive(Debug, Serialize)]
pub struct Entry {
    pub pkg_name: String,
    /// Only emitted when a package states one. It exists to group packages
    /// whose directory name differs from the name they install under, so
    /// repeating the name here would carry no information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkg_family: Option<String>,
    pub pkg_type: Option<String>,
    pub description: Option<String>,
    pub version: String,
    /// Upstream publication date, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub download_url: String,
    /// Bytes. Clients format this for display; shipping a pre-formatted
    /// string as well just meant two fields that could disagree.
    pub size: Option<u64>,
    pub src_url: Vec<String>,
    pub homepage: Vec<String>,
    pub license: Vec<String>,
    pub maintainer: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub note: Vec<String>,
    pub category: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repology: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shasum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bsum: Option<String>,
    /// Side files to install alongside the artifact, typically a licence the
    /// artifact itself does not carry. Each carries a hash unless its recipe
    /// opted out, which licences do because they are served from a branch.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<ExtraFile>,
    /// Everything the package installs out of its artifact, as archive path to
    /// installed name. Empty means the recipe named nothing, so the whole
    /// artifact is the package.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileMapping>,
}

/// One file taken out of the artifact, as published in the index.
#[derive(Debug, Clone, Serialize)]
pub struct FileMapping {
    pub source: String,
    pub to: String,
    /// Names this file is exposed under in the bin directory. Empty means the
    /// file is installed but not linked, as a licence is.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub link_as: Vec<String>,
}

/// The names a `provides` entry puts in the bin directory.
///
/// The markers belong to `provides` alone, which is where soar's legacy format
/// encodes link names. They are resolved here so a file mapping carries plain
/// names and nothing downstream has to parse them again.
fn provide_link_names(provide: &str) -> Vec<String> {
    let provide = provide.strip_prefix('@').unwrap_or(provide);
    for (sep, keep_both) in [("==", true), ("=>", false), (":", false)] {
        if let Some((name, target)) = provide.split_once(sep) {
            return if keep_both {
                vec![name.trim().to_string(), target.trim().to_string()]
            } else {
                vec![target.trim().to_string()]
            };
        }
    }
    vec![provide.trim().to_string()]
}

/// A pinned side file as published in the index.
#[derive(Debug, Clone, Serialize)]
pub struct ExtraFile {
    pub url: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blake3: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// Whether an installed file is a desktop-integration resource rather than an
/// executable.
///
/// soar treats a non-empty `binaries` as the complete list of things to link,
/// so one icon or desktop entry in there stops the actual binary being found.
fn is_resource(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    matches!(ext.as_deref(), Some("desktop" | "png" | "svg" | "xpm" | "ico"))
}

/// Expand the two template variables an install path may carry.
fn expand_arch(s: &str, version: &str, arch: &str) -> String {
    s.replace("${version}", version).replace("${arch}", arch)
}

/// Notes a user needs told, and nothing else.
///
/// Provenance and portability restate `src_url` and `type`, which the entry
/// already carries, so they are not repeated here as prose. Needing something
/// from the host is the exception: it is a limitation rather than a property,
/// and there is no other field carrying it.
fn render_notes(p: &PkgToml) -> Vec<String> {
    let mut out = Vec::new();
    if !p.pkg.portable {
        out.push(match &p.pkg.portable_reason {
            Some(why) => format!("[NOT PORTABLE] {why}"),
            None => "[NOT PORTABLE]".to_string(),
        });
    }
    out.extend(p.pkg.note.iter().cloned());
    out
}

/// Generate every index entry for `host`.
pub fn generate(root: &Path, host: &str) -> (Vec<Entry>, Vec<String>) {
    let (packages, errors) = tree::load(root);
    let mut out = Vec::new();

    for pkg in &packages {
        let p = &pkg.pkg;
        if p.pkg.disabled || !p.host.supported.iter().any(|h| h == host) {
            continue;
        }
        let fam = p.pkg.family.clone();
        let srcs = p.src_urls();

        for v in &pkg.versions {
            let Some(url) = v.url.get(host) else { continue };

            // The version file may override pkg.toml; these are the fields
            // that realistically change between releases.
            let provides = v.provides.clone().unwrap_or_else(|| p.pkg.provides.clone());
            let note_src = v.note.clone();

            let size = v.size.get(host).copied();
            // soar's index has a bsum column only, so the blake3 digest is
            // what actually reaches a client. sha256 rides along for
            // cross-checking against upstream checksum files.
            let bsum = v.blake3.get(host).cloned();
            let shasum = v.sha256.get(host).cloned();

            // No ghcr_* fields are emitted: soar prefers an OCI source over
            // download_url and requires a per-artifact signature there, which
            // the upstream-plus-hash model does not produce. `ghcr` stays in
            // pkg.toml because it defines the package identity namespace.
            let arch_for_host = {
                let raw = host.split('-').next().unwrap_or(host);
                p.arch.get(raw).cloned().unwrap_or_else(|| raw.to_string())
            };
            // Licences and docs are not executables, and a file already at
            // the archive root under its own name needs no mapping.

            // The recipe's install map in full, not just its executables. A
            // recipe naming `*` means the whole artifact, which is published
            // as no mapping at all rather than as a wildcard to interpret.
            let files: Vec<FileMapping> = p
                .source
                .as_ref()
                .filter(|src| !src.install.keys().any(|from| from == "*"))
                .map(|src| {
                    src.install
                        .iter()
                        .map(|(from, to)| {
                            let installed = to.trim().to_string();
                            // A file is linked under whatever `provides` says
                            // it is called. Matched on the file name, since a
                            // target is a path: bin/fd is still called fd.
                            let name = installed.rsplit('/').next().unwrap_or(&installed);
                            let link_as: Vec<String> = provides
                                .iter()
                                .filter(|q| {
                                    let names = provide_link_names(q);
                                    names.iter().any(|n| n == name)
                                        || q.strip_prefix('@').unwrap_or(q) == name
                                })
                                .flat_map(|q| provide_link_names(q))
                                .collect();
                            FileMapping {
                                source: expand_arch(from, &v.version, &arch_for_host)
                                    .trim_start_matches("*/")
                                    .to_string(),
                                to: installed,
                                link_as,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();

            let extras: Vec<ExtraFile> = v
                .extra
                .iter()
                // A side file pinned per host belongs only to that host's
                // index; one without a host applies to all of them.
                .filter(|e| e.host.as_deref().is_none_or(|h| h == host))
                .map(|e| ExtraFile {
                    url: e.url.clone(),
                    to: e.to.clone(),
                    blake3: e.blake3.clone(),
                    sha256: e.sha256.clone(),
                })
                .collect();

            {
                let mut note = render_notes(p);
                if let Some(n) = &note_src {
                    note = n.clone();
                }
                out.push(Entry {
                    // Named by `name`, not by what it installs: ripgrep
                    // ships `rg`.
                    pkg_name: p.pkg.name.clone(),
                    pkg_family: fam.clone(),
                    pkg_type: p.pkg.kind.clone(),
                    description: p.pkg.description.clone(),
                    version: v.version.clone(),
                    date: v.date.clone(),
                    download_url: url.clone(),
                    size,
                    src_url: srcs.clone(),
                    homepage: p.pkg.homepage.clone(),
                    license: p.pkg.license.clone(),
                    maintainer: p.pkg.maintainer.clone(),
                    note,
                    category: p.pkg.category.clone(),
                    repology: p.pkg.repology.clone(),
                    shasum: shasum.clone(),
                    bsum: bsum.clone(),
                    extra: extras.clone(),
                    files: files.clone(),
                });
            }
        }
    }

    out.sort_by(|a, b| a.pkg_name.cmp(&b.pkg_name));
    (out, errors)
}
