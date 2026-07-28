//! Metadata generation from a ports tree.
//!
//! This is a pure function: tree in, index out. No network, no database, no
//! build state. Running it on a clean checkout reproduces the same bytes,
//! which is what makes the published index auditable.

use std::path::Path;

use serde::Serialize;

use crate::port::{model::PkgToml, tree};


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
    pub provides: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repology: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shasum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bsum: Option<String>,
    /// Where each executable lives inside the artifact. Only emitted when a
    /// file has to be renamed or is not at the archive root, since soar
    /// otherwise finds it by package name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub binaries: Vec<Binary>,
    /// Side files to install alongside the artifact, each pinned. Typically a
    /// licence the artifact itself does not carry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<ExtraFile>,
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

/// One executable inside the artifact, as published in the index.
#[derive(Debug, Clone, Serialize)]
pub struct Binary {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_as: Option<String>,
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
            let binaries: Vec<Binary> = p
                .source
                .as_ref()
                .map(|src| {
                    src.install
                        .iter()
                        .filter(|(from, to)| {
                            let base = from.rsplit('/').next().unwrap_or(from);
                            // An entry is worth publishing when the file has
                            // to be renamed, or when it is nested rather than
                            // at the artifact root: soar looks at the root by
                            // package name and would not find bin/<name>.
                            let nested = from.trim_start_matches("*/").contains('/');
                            (base != *to || nested)
                                && !to.eq_ignore_ascii_case("LICENSE")
                                && !is_resource(to)
                                && *from != "*"
                        })
                        .map(|(from, to)| Binary {
                            // The index is generated per host, so templates
                            // are expanded here rather than shipped for the
                            // client to resolve.
                            // Published as written against the archive. An
                            // archive with one top-level directory has it
                            // promoted away before binaries are resolved, so
                            // the client retries without the leading
                            // component; stripping it here instead would
                            // discard the only thing telling two
                            // architectures apart in a multi-arch archive.
                            source: expand_arch(from, &v.version, &arch_for_host)
                                .trim_start_matches("*/")
                                .to_string(),
                            // Strip soar's provides markers; link_as is a
                            // plain filename.
                            link_as: Some(
                                to.split("==").next().unwrap_or(to)
                                    .split("=>").next().unwrap_or(to)
                                    .trim().to_string(),
                            ),
                        })
                        .collect()
                })
                .unwrap_or_default();

            // Only pinned side files are published: an unhashed fetch would
            // be an unverified download, which is the thing this format exists
            // to avoid.
            let extras: Vec<ExtraFile> = v
                .extra
                .iter()
                .filter(|e| e.blake3.is_some() || e.sha256.is_some())
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
                let prov = provides.clone();
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
                    download_url: url.clone(),
                    size,
                    src_url: srcs.clone(),
                    homepage: p.pkg.homepage.clone(),
                    license: p.pkg.license.clone(),
                    maintainer: p.pkg.maintainer.clone(),
                    note,
                    category: p.pkg.category.clone(),
                    provides: prov,
                    repology: p.pkg.repology.clone(),
                    shasum: shasum.clone(),
                    bsum: bsum.clone(),
                    binaries: binaries.clone(),
                    extra: extras.clone(),
                });
            }
        }
    }

    out.sort_by(|a, b| a.pkg_name.cmp(&b.pkg_name));
    (out, errors)
}
