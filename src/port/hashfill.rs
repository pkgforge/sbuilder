//! Hash artifacts the forge API could not vouch for.
//!
//! Releases predating GitHub's asset `digest` field, and non-GitHub sources
//! such as GitLab generic packages, have no reported hash. Those artifacts are
//! streamed once and hashed locally. Nothing is written to disk: a pinned URL
//! without a hash is the one state the tree must never be left in.

use std::{collections::BTreeMap, fs, path::Path};

use futures::StreamExt;
use sha2::{Digest, Sha256};

/// A side file declared in pkg.toml that this version has not pinned yet.
pub struct ExtraGap {
    /// Which host this gap is for, when the URL is arch-dependent.
    pub host: Option<String>,
    pub path: std::path::PathBuf,
    /// URL the client will fetch it from.
    pub url: String,
    pub to: String,
    /// Set when the file is vendored here: hash it from disk instead of
    /// fetching a copy of something we already have.
    pub local: Option<std::path::PathBuf>,
    /// Whether this file's content is pinned. False means it is recorded
    /// without a hash and never downloaded here.
    pub verify: bool,
}

/// Side files a version file does not list yet.
///
/// These are not hashed. Every one is a licence, which is documentation rather
/// than something that runs, and they are served from a branch rather than a
/// tag: a pinned hash would turn an upstream copyright-year bump into a failed
/// download for everyone installing that version.
pub fn extra_gaps(root: &Path) -> Vec<ExtraGap> {
    let (packages, _) = crate::port::tree::load(root);
    let mut out = Vec::new();
    for p in packages {
        if p.pkg.pkg.disabled || p.pkg.extra.is_empty() {
            continue;
        }
        let mut paths: Vec<_> = fs::read_dir(&p.dir)
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
        for (v, path) in p.versions.iter().zip(paths) {
            for e in &p.pkg.extra {
                // A side file whose URL names an architecture is a different
                // file on every host, so it is pinned once per host rather
                // than once per package.
                let hosts: Vec<Option<String>> = match &e.url {
                    Some(u) if u.contains("${arch}") => p
                        .pkg
                        .host
                        .supported
                        .iter()
                        .map(|h| Some(h.clone()))
                        .collect(),
                    _ => vec![None],
                };
                for host in hosts {
                    let pinned = v
                        .extra
                        .iter()
                        // An unhashed file is done once it is listed at all;
                        // waiting for a hash it will never get would append a
                        // duplicate block on every run.
                        .any(|x| {
                            x.to == e.to
                                && x.host == host
                                && (!e.verify() || x.blake3.is_some())
                        });
                    if pinned {
                        continue;
                    }
                    // A vendored licence resolves to this repository rather
                    // than upstream, which is the point: some hosts rate-limit
                    // and some upstreams are gone.
                    let (url, local) = match (&e.url, &e.license) {
                        (Some(u), _) => (u.replace("${version}", &v.version), None),
                        (None, Some(spdx)) => (
                            format!(
                                "https://raw.githubusercontent.com/pkgforge/soarpkgs/main/licenses/{spdx}.txt"
                            ),
                            Some(root.join("licenses").join(format!("{spdx}.txt"))),
                        ),
                        _ => continue,
                    };
                    // `${arch}` is whatever upstream calls the architecture,
                    // which is not always what the host is called.
                    let url = match &host {
                        Some(h) => {
                            let raw = h.split('-').next().unwrap_or(h);
                            let arch =
                                p.pkg.arch.get(raw).cloned().unwrap_or_else(|| raw.to_string());
                            url.replace("${arch}", &arch)
                        }
                        None => url,
                    };
                    out.push(ExtraGap {
                        host,
                        path: path.clone(),
                        url,
                        to: e.to.clone(),
                        local,
                        verify: e.verify(),
                    });
                }
            }
        }
    }
    out
}

/// A version file with at least one unhashed URL.
pub struct Gap {
    pub path: std::path::PathBuf,
    pub host: String,
    pub url: String,
}

/// Find every pinned URL lacking a blake3 hash.
///
/// blake3 is the digest soar verifies against, and it cannot be obtained from
/// a forge API, so any entry without one has to be downloaded.
pub fn gaps(root: &Path) -> Vec<Gap> {
    let (packages, _) = crate::port::tree::load(root);
    let mut out = Vec::new();
    for p in packages {
        let mut paths: Vec<_> = fs::read_dir(&p.dir)
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
        for (v, path) in p.versions.iter().zip(paths) {
            for (host, url) in &v.url {
                if !v.blake3.contains_key(host) {
                    out.push(Gap { path: path.clone(), host: host.clone(), url: url.clone() });
                }
            }
        }
    }
    out
}

/// Hash a file already on disk, for content vendored in this repository.
pub fn digests_local(path: &Path) -> Result<(String, String, u64), String> {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut sha = Sha256::new();
    sha.update(&data);
    let sha_hex: String = sha.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((
        blake3::hash(&data).to_hex().to_string(),
        sha_hex,
        data.len() as u64,
    ))
}

/// Stream a URL once, returning (blake3, sha256, bytes). The body is
/// discarded, so memory and disk stay flat regardless of artifact size.
pub async fn digests(client: &reqwest::Client, url: &str) -> Result<(String, String, u64), String> {
    let resp = client
        .get(url)
        .header("User-Agent", "sbuild-port")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("http {}", resp.status().as_u16()));
    }
    let mut sha = Sha256::new();
    let mut b3 = blake3::Hasher::new();
    let mut n = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        n += chunk.len() as u64;
        sha.update(&chunk);
        b3.update(&chunk);
    }
    let sha_hex: String = sha.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((b3.finalize().to_hex().to_string(), sha_hex, n))
}

/// Append resolved side files to a version file.
pub fn merge_extras(
    path: &Path,
    new: &[(String, String, Option<String>, Option<String>, Option<String>)],
) -> Result<(), String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut s = raw.trim_end().to_string();
    for (url, to, host, b3, sha) in new {
        s.push_str("\n\n[[extra]]");
        s.push_str(&format!("\nurl    = {url:?}"));
        s.push_str(&format!("\nto     = {to:?}"));
        if let Some(h) = host {
            s.push_str(&format!("\nhost   = {h:?}"));
        }
        if let (Some(b3), Some(sha)) = (b3, sha) {
            s.push_str(&format!("\nblake3 = {b3:?}\nsha256 = {sha:?}"));
        }
    }
    s.push('\n');
    fs::write(path, s).map_err(|e| e.to_string())
}

/// Merge freshly computed hashes and sizes into a version file, preserving
/// everything above the digest tables.
pub fn merge(path: &Path, new: &BTreeMap<String, (String, String, u64)>) -> Result<(), String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: crate::port::model::VersionToml =
        toml::from_str(&raw).map_err(|e| e.to_string())?;

    let mut b3s = v.blake3.clone();
    let mut shas = v.sha256.clone();
    let mut sizes = v.size.clone();
    for (host, (b3, sha, size)) in new {
        b3s.insert(host.clone(), b3.clone());
        shas.insert(host.clone(), sha.clone());
        sizes.insert(host.clone(), *size);
    }

    let head = raw
        .split("\n[blake3]")
        .next()
        .unwrap_or(&raw)
        .split("\n[hash]")
        .next()
        .unwrap_or(&raw)
        .split("\n[sha256]")
        .next()
        .unwrap_or(&raw)
        .split("\n[size]")
        .next()
        .unwrap_or(&raw)
        .trim_end()
        .to_string();

    let mut s = head;
    if !b3s.is_empty() {
        let w = b3s.keys().map(|k| k.len()).max().unwrap_or(0);
        s.push_str("\n\n[blake3]\n");
        for (h, val) in &b3s {
            s.push_str(&format!("{:<w$} = {:?}\n", h, val, w = w));
        }
    }
    if !shas.is_empty() {
        let w = shas.keys().map(|k| k.len()).max().unwrap_or(0);
        s.push_str("\n[sha256]\n");
        for (h, val) in &shas {
            s.push_str(&format!("{:<w$} = {:?}\n", h, val, w = w));
        }
    }
    if !sizes.is_empty() {
        let w = sizes.keys().map(|k| k.len()).max().unwrap_or(0);
        s.push_str("\n[size]\n");
        for (h, val) in &sizes {
            s.push_str(&format!("{:<w$} = {}\n", h, val, w = w));
        }
    }
    fs::write(path, s).map_err(|e| e.to_string())
}
