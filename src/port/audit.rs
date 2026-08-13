//! Verify that every install-map path resolves inside its real archive.
//!
//! Static checks cannot catch a path that simply is not in the tarball, which
//! is the failure that ships a broken package: the artifact downloads, its
//! hash verifies, and the file the map points at does not exist. The only way
//! to know is to fetch the archive and list it.
//!
//! Listing shells out to `tar` and `unzip` rather than linking a decompressor
//! for every format upstreams use. This is a maintainer tool, never something
//! a client runs.

use std::{path::Path, process::Command};

use futures::StreamExt;

use crate::port::{resolve::expand, tree};

/// What the audit concluded about one package on one host.
#[derive(Debug)]
pub enum Outcome {
    /// Every install path matched at least one archive member.
    Ok,
    /// Paths that matched nothing. These ship a broken package.
    Missing(Vec<String>),
    /// Single-file compression (`.gz`, `.bz2`) has no member list, so the
    /// install path cannot be checked this way.
    Unlistable,
    Failed(String),
}

#[derive(Debug)]
pub struct Finding {
    pub package: String,
    pub host: String,
    pub outcome: Outcome,
}

impl Finding {
    pub fn is_problem(&self) -> bool {
        matches!(self.outcome, Outcome::Missing(_) | Outcome::Failed(_))
    }
}

/// One unit of work: a package's artifact for a single host.
struct Job {
    package: String,
    host: String,
    url: String,
    version: String,
    arch: String,
    install: Vec<String>,
}

fn list_members(path: &Path) -> Option<Vec<String>> {
    // GNU tar auto-detects gz/xz/bz2/zst, so try it first and fall back to
    // zip; `file` mislabels some zips (OOXML), so never trust it to choose.
    for (cmd, args) in [("tar", vec!["tf"]), ("unzip", vec!["-Z1"])] {
        let out = Command::new(cmd)
            .args(&args)
            .arg(path)
            .output()
            .ok()?;
        if out.status.success() {
            let members: Vec<String> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect();
            if !members.is_empty() {
                return Some(members);
            }
        }
    }
    None
}

fn matches(pattern: &str, members: &[String]) -> bool {
    let pat = glob::Pattern::new(pattern).ok();
    members.iter().any(|m| {
        m == pattern
            || m.trim_end_matches('/') == pattern
            || m.ends_with(&format!("/{pattern}"))
            || pat.as_ref().is_some_and(|p| p.matches(m))
    })
}

async fn audit_one(client: &reqwest::Client, job: Job) -> Finding {
    let mut tmp = match tempfile::NamedTempFile::new() {
        Ok(t) => t,
        Err(e) => {
            return Finding {
                package: job.package,
                host: job.host,
                outcome: Outcome::Failed(e.to_string()),
            }
        }
    };

    let fetched = async {
        use std::io::Write;
        let resp = client
            .get(&job.url)
            .header("User-Agent", "sbuild-audit")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("http {}", resp.status().as_u16()));
        }
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            tmp.as_file_mut().write_all(&chunk).map_err(|e| e.to_string())?;
        }
        tmp.as_file_mut().flush().map_err(|e| e.to_string())
    }
    .await;

    if let Err(e) = fetched {
        return Finding { package: job.package, host: job.host, outcome: Outcome::Failed(e) };
    }

    let outcome = match list_members(tmp.path()) {
        None => Outcome::Unlistable,
        Some(members) => {
            let missing: Vec<String> = job
                .install
                .iter()
                .map(|k| expand(k, &job.version, &job.arch, &job.package))
                .filter(|p| !matches(p, &members))
                .collect();
            if missing.is_empty() { Outcome::Ok } else { Outcome::Missing(missing) }
        }
    };
    Finding { package: job.package, host: job.host, outcome }
}

/// Audit every package with an install map, optionally restricted by name.
pub async fn run(
    root: &Path,
    hosts: &[String],
    only: &[String],
    jobs: usize,
) -> Result<Vec<Finding>, String> {
    let (packages, _) = tree::load(root);
    let mut work = Vec::new();

    for p in &packages {
        let name = p.pkg.family().to_string();
        if p.pkg.pkg.disabled || (!only.is_empty() && !only.contains(&name)) {
            continue;
        }
        let Some(src) = &p.pkg.source else { continue };
        // An install map is the only thing worth auditing. Whether the
        // artifact is an archive is decided by listing it, not by a field:
        // soar sniffs magic bytes, so nothing else needs to be told.
        if src.install.is_empty() {
            continue;
        }
        let Some(v) = p.versions.last() else { continue };
        for host in hosts {
            let Some(url) = v.url.get(host) else { continue };
            let raw = host.split('-').next().unwrap_or(host);
            work.push(Job {
                package: name.clone(),
                host: host.clone(),
                url: url.clone(),
                version: v.version.clone(),
                arch: p.pkg.arch.get(raw).cloned().unwrap_or_else(|| raw.to_string()),
                install: src.install
                    .entries(host)
                    .iter()
                    .filter_map(|e| e.from.clone())
                    .collect(),
            });
        }
    }

    let client = reqwest::Client::builder()
        .user_agent("sbuild-audit")
        .build()
        .map_err(|e| e.to_string())?;

    println!("auditing {} artifacts with {jobs} concurrent downloads ...", work.len());
    let total = work.len();
    let mut findings = Vec::new();
    let mut stream = futures::stream::iter(
        work.into_iter().map(|j| audit_one(&client, j)),
    )
    .buffer_unordered(jobs);

    let mut done = 0;
    while let Some(f) = stream.next().await {
        done += 1;
        findings.push(f);
        if done % 25 == 0 {
            eprintln!("  ... {done}/{total}");
        }
    }
    findings.sort_by(|a, b| a.package.cmp(&b.package).then(a.host.cmp(&b.host)));
    Ok(findings)
}
