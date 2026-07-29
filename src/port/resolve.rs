//! Version and artifact resolution: the only part of the pipeline that needs
//! the network.
//!
//! Given a package's update policy, find the current upstream version, select
//! the matching release asset per host, and record the resolved URL, hash and
//! size. The result is written as a pinned version file, after which nothing
//! else in the system has to trust upstream again.

use indexmap::IndexMap;

use serde::Deserialize;

use crate::port::model::{PkgToml, Source, UrlSpec, Update};

const API: &str = "https://api.github.com";

/// A release asset as reported by the forge API.
#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: Option<u64>,
    /// GitHub reports `sha256:...` for assets uploaded since the field
    /// existed. Older releases have none and must be hashed by download.
    pub digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: Option<String>,
    /// When upstream published the release. Recorded because a version built
    /// from a commit carries no order of its own.
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Tag {
    name: String,
}

/// What resolution produced for one package.
pub struct Resolved {
    pub version: String,
    /// Upstream publication date, when the forge reports one.
    pub date: Option<String>,
    /// Insertion-ordered to follow `host.supported`, so re-resolving an
    /// unchanged package produces an unchanged file.
    pub urls: IndexMap<String, String>,
    /// sha256 only: the forge reports it, blake3 needs a download.
    pub hashes: IndexMap<String, String>,
    /// blake3 is never resolved here; it is carried forward from an existing
    /// pin so re-resolving cannot discard what hashfill downloaded.
    pub blake3: IndexMap<String, String>,
    pub sizes: IndexMap<String, u64>,
    /// Hosts that could not be resolved, with a reason.
    pub missing: Vec<String>,
}

/// Substitute the three template variables. Deliberately not an expression
/// language: anything more complex belongs in an explicit per-host table.
pub fn expand(tpl: &str, version: &str, arch: &str, name: &str) -> String {
    tpl.replace("${version}", version)
        .replace("${arch}", arch)
        .replace("${name}", name)
}

/// Strip the decoration upstreams put around a version in their tags.
pub fn clean_version(tag: &str, u: &Update) -> String {
    let mut v = tag.to_string();
    if u.tag_suffix.as_deref() == Some("strip") {
        v = v.split('@').next().unwrap_or(&v).to_string();
    }
    if let Some(p) = &u.tag_prefix {
        if let Some(rest) = v.strip_prefix(p.as_str()) {
            v = rest.to_string();
        }
    }
    if let Some(p) = &u.strip_prefix {
        if let Some(rest) = v.strip_prefix(p.as_str()) {
            v = rest.to_string();
        }
    }
    // A bare leading `v` only counts when a digit follows it.
    let bytes = v.as_bytes();
    if bytes.first() == Some(&b'v') && bytes.get(1).is_some_and(|c| c.is_ascii_digit()) {
        v = v.trim_start_matches('v').to_string();
    }
    v
}

fn is_sidecar(name: &str) -> bool {
    let n = name.to_lowercase();
    [".sig", ".asc", ".zsync", ".txt", ".pem", ".sbom", ".json", ".intoto", ".jsonl"]
        .iter()
        .any(|e| n.ends_with(e))
        || n.rsplit('.').next().is_some_and(|e| {
            e.starts_with("sha") && e[3..].chars().all(|c| c.is_ascii_digit() || c == 's' || c == 'u' || c == 'm')
        })
}

/// Choose the artifact for one host from a release's assets.
pub fn pick_asset<'a>(
    assets: &'a [Asset],
    src: &Source,
    version: &str,
    arch: &str,
    name: &str,
) -> Option<&'a Asset> {
    let mut cands: Vec<&Asset> = assets.iter().collect();

    if let Some(g) = &src.glob {
        let pat = expand(g, version, arch, name).to_lowercase();
        let pattern = glob::Pattern::new(&pat).ok()?;
        cands.retain(|a| pattern.matches(&a.name.to_lowercase()));
    } else if !src.r#match.is_empty() {
        for tok in &src.r#match {
            let t = expand(tok, version, arch, name).to_lowercase();
            cands.retain(|a| a.name.to_lowercase().contains(&t));
        }
    }
    for tok in &src.exclude {
        let t = expand(tok, version, arch, name).to_lowercase();
        cands.retain(|a| !a.name.to_lowercase().contains(&t));
    }
    cands.retain(|a| !is_sidecar(&a.name));

    // Shortest name wins; upstreams decorate variants, not the plain artifact.
    cands.sort_by(|a, b| a.name.len().cmp(&b.name.len()).then(a.name.cmp(&b.name)));
    cands.into_iter().next()
}

/// Marks an error as "the forge is refusing everything", so a caller can stop
/// rather than repeat the same failure for every remaining package.
pub const RATE_LIMITED: &str = "rate limited";

async fn get_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
    token: &str,
) -> Result<T, String> {
    let mut req = client.get(url).header("Accept", "application/vnd.github+json");
    if !token.is_empty() && url.starts_with(API) {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        // GitHub answers 403 or 429 once the quota is gone, and says so only
        // in the headers. Without them every package looks like its own
        // failure rather than one shared cause.
        let exhausted = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == "0");
        if exhausted || status == 429 {
            let reset = resp
                .headers()
                .get("x-ratelimit-reset")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i64>().ok());
            return Err(match reset {
                Some(at) => format!("{RATE_LIMITED} (quota resets at unix {at})"),
                None => RATE_LIMITED.to_string(),
            });
        }
        return Err(format!("api {status}"));
    }
    resp.json::<T>().await.map_err(|e| e.to_string())
}

/// Resolve the current version and per-host artifacts for one package.
pub async fn resolve(
    client: &reqwest::Client,
    pkg: &PkgToml,
    token: &str,
) -> Result<Resolved, String> {
    let upd = pkg.update.as_ref().ok_or("no update policy")?;
    let src = pkg.source.as_ref().ok_or("no source")?;
    let name = &pkg.pkg.name;

    let mut assets: Vec<Asset> = Vec::new();
    let mut published: Option<String> = None;
    let tag = match upd.strategy.as_str() {
        "html-regex" => {
            let body = client
                .get(&upd.repo)
                .header("User-Agent", "sbuild-port")
                .send()
                .await
                .map_err(|e| e.to_string())?
                .text()
                .await
                .map_err(|e| e.to_string())?;
            let pat = upd.pattern.as_deref().ok_or("html-regex needs a pattern")?;
            let re = regex::Regex::new(pat).map_err(|e| e.to_string())?;
            re.captures(&body)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .ok_or("pattern matched nothing")?
        }
        "gitlab-tags" => {
            let url = format!(
                "https://gitlab.com/api/v4/projects/{}/repository/tags?per_page=5&order_by=version",
                upd.repo
            );
            let tags: Vec<Tag> = get_json(client, &url, token).await?;
            tags.first().map(|t| t.name.clone()).ok_or("no tags")?
        }
        "github-tags" => {
            let url = format!("{API}/repos/{}/tags", upd.repo);
            let tags: Vec<Tag> = get_json(client, &url, token).await?;
            tags.first().map(|t| t.name.clone()).ok_or("no tags")?
        }
        _ => {
            // A repository that publishes for several packages has no single
            // "latest", so filter by tag prefix when one is given.
            let rel: Release = if let Some(prefix) = &upd.tag_prefix {
                let url = format!("{API}/repos/{}/releases?per_page=100", upd.repo);
                let rels: Vec<Release> = get_json(client, &url, token).await?;
                rels.into_iter()
                    .find(|r| {
                        r.tag_name.as_deref().is_some_and(|t| t.starts_with(prefix.as_str()))
                    })
                    .ok_or("no release matching tag-prefix")?
            } else {
                let url = format!("{API}/repos/{}/releases/latest", upd.repo);
                get_json(client, &url, token).await?
            };
            assets = rel.assets;
            published = rel.published_at.clone();
            rel.tag_name.ok_or("no tag_name")?
        }
    };

    let version = clean_version(&tag, upd);
    if version.is_empty() {
        return Err("empty version".into());
    }

    let hosts = if pkg.host.supported.is_empty() {
        vec!["x86_64-linux".to_string()]
    } else {
        pkg.host.supported.clone()
    };

    let mut out = Resolved {
        version: version.clone(),
        date: published,
        urls: IndexMap::new(),
        hashes: IndexMap::new(),
        blake3: IndexMap::new(),
        sizes: IndexMap::new(),
        missing: Vec::new(),
    };

    for host in hosts {
        let raw = host.split('-').next().unwrap_or(&host).to_string();
        let arch = pkg.arch.get(&raw).cloned().unwrap_or(raw);

        let (url, asset) = match &src.url {
            Some(UrlSpec::PerHost(m)) => {
                let Some(tpl) = m.get(&host) else {
                    out.missing.push(format!("{host}:no-url"));
                    continue;
                };
                let u = expand(tpl, &version, &arch, name);
                let a = assets.iter().find(|a| a.browser_download_url == u);
                (Some(u), a)
            }
            Some(UrlSpec::Template(tpl)) => {
                let u = expand(tpl, &version, &arch, name);
                let base = u.rsplit('/').next().unwrap_or_default().to_string();
                let a = assets
                    .iter()
                    .find(|a| a.browser_download_url == u || a.name == base);
                (Some(u), a)
            }
            None => {
                let a = pick_asset(&assets, src, &version, &arch, name);
                (a.map(|a| a.browser_download_url.clone()), a)
            }
        };

        // Always take the URL from the asset that was actually matched.
        // Emitting an expanded template while hashing a differently-matched
        // asset can pin a correct hash against a URL that 404s.
        let url = match asset {
            Some(a) => Some(a.browser_download_url.clone()),
            None => url,
        };
        let Some(url) = url else {
            out.missing.push(format!("{host}:no-asset"));
            continue;
        };
        if url.contains("${") {
            out.missing.push(format!("{host}:unexpanded"));
            continue;
        }

        out.urls.insert(host.clone(), url);
        if let Some(a) = asset {
            if let Some(s) = a.size {
                out.sizes.insert(host.clone(), s);
            }
            match &a.digest {
                Some(d) => {
                    let bare = d.strip_prefix("sha256:").unwrap_or(d);
                    out.hashes.insert(host.clone(), bare.to_string());
                }
                None => out.missing.push(format!("{host}:no-digest")),
            }
        }
    }

    if out.urls.is_empty() {
        return Err("no asset matched any host".into());
    }
    Ok(out)
}

/// Carry hashes forward from an existing pin.
///
/// Forges that report no digest (GitLab, older GitHub releases) would
/// otherwise lose the hash that `hashfill` computed. Only reuse a hash when
/// the URL is byte-identical, so a changed artifact can never keep a stale one.
pub fn carry_forward(r: &mut Resolved, existing: &crate::port::model::VersionToml) {
    if existing.version != r.version {
        return;
    }
    for (host, url) in &r.urls {
        if r.hashes.contains_key(host) && r.blake3.contains_key(host) {
            continue;
        }
        if existing.url.get(host).map(|u| u == url).unwrap_or(false) {
            if let Some(h) = existing.sha256.get(host) {
                r.hashes.insert(host.clone(), h.clone());
            }
            if let Some(b) = existing.blake3.get(host) {
                r.blake3.insert(host.clone(), b.clone());
            }
            if let Some(s) = existing.size.get(host) {
                r.sizes.insert(host.clone(), *s);
            }
        }
    }
}

/// Render a pinned version file. Keys are padded so diffs stay readable.
pub fn render(r: &Resolved) -> String {
    let mut s = format!("version = {:?}\n", r.version);
    if let Some(date) = &r.date {
        s.push_str(&format!("date    = {date:?}\n"));
    }
    let w = r.urls.keys().map(|k| k.len()).max().unwrap_or(0);
    s.push_str("\n[url]\n");
    for (h, u) in &r.urls {
        s.push_str(&format!("{:<w$} = {:?}\n", h, u, w = w));
    }
    if !r.blake3.is_empty() {
        let w = r.blake3.keys().map(|k| k.len()).max().unwrap_or(0);
        s.push_str("\n[blake3]\n");
        for (h, v) in &r.blake3 {
            s.push_str(&format!("{:<w$} = {:?}\n", h, v, w = w));
        }
    }
    if !r.hashes.is_empty() {
        let w = r.hashes.keys().map(|k| k.len()).max().unwrap_or(0);
        s.push_str("\n[sha256]\n");
        for (h, v) in &r.hashes {
            s.push_str(&format!("{:<w$} = {:?}\n", h, v, w = w));
        }
    }
    if !r.sizes.is_empty() {
        let w = r.sizes.keys().map(|k| k.len()).max().unwrap_or(0);
        s.push_str("\n[size]\n");
        for (h, v) in &r.sizes {
            s.push_str(&format!("{:<w$} = {}\n", h, v, w = w));
        }
    }
    s
}
