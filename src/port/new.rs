//! Scaffolding for a new package.
//!
//! Writes a `pkg.toml` with the update policy and source selector filled in.
//! No version is pinned here: run `port resolve` afterwards, which is what
//! records the URL and hash.

use std::{fs, path::Path};

/// What kind of artifact the upstream release publishes.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    AppImage,
    Static,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "appimage" => Ok(Kind::AppImage),
            "static" => Ok(Kind::Static),
            other => Err(format!("unknown type {other:?}, expected appimage or static")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Kind::AppImage => "appimage",
            Kind::Static => "static",
        }
    }
}

pub struct Scaffold<'a> {
    pub name: &'a str,
    pub repo: &'a str,
    pub kind: Kind,
    pub description: Option<&'a str>,
    pub maintainer: Option<&'a str>,
    /// Upstream tags carrying a build suffix, e.g. `1.2.3@2026-01-01_1234`.
    pub tag_suffix_strip: bool,
}

fn arr(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| format!("{s:?}")).collect();
    format!("[{}]", inner.join(", "))
}

/// Render a `pkg.toml`. Fields a human must still fill are left as TODO so
/// validation and review catch them rather than them being silently wrong.
pub fn render(s: &Scaffold) -> String {
    let owner = s.repo.split('/').next().unwrap_or("upstream");
    let mut out = String::from("[pkg]\n");
    out.push_str(&format!("name        = {:?}\n", s.name));
    out.push_str(&format!("id          = {:?}\n", format!("{owner}.{}", s.name)));
    out.push_str(&format!("type        = {:?}\n", s.kind.as_str()));
    out.push_str(&format!(
        "description = {:?}\n",
        s.description.unwrap_or("TODO: one-line description")
    ));
    out.push_str(&format!(
        "homepage    = {}\n",
        arr(&[format!("https://github.com/{}", s.repo)])
    ));
    out.push_str("license     = [\"TODO\"]\n");
    out.push_str(&format!(
        "maintainer  = {}\n",
        arr(&[s.maintainer.unwrap_or("TODO <you@example.com>").to_string()])
    ));
    out.push_str("category    = [\"TODO\"]\n");
    out.push_str("tag         = [\"TODO\"]\n");
    out.push_str("repology    = [\"TODO\"]\n");

    out.push_str("\n[host]\nsupported = [\"x86_64-linux\", \"aarch64-linux\"]\n");

    out.push_str("\n[update]\nstrategy = \"github-releases\"\n");
    out.push_str(&format!("repo     = {:?}\n", s.repo));
    if s.tag_suffix_strip {
        out.push_str("tag-suffix = \"strip\"\n");
    }

    out.push_str("\n[source]\n");
    out.push_str(&format!("github = {:?}\n", s.repo));
    match s.kind {
        Kind::AppImage => out.push_str("glob   = \"*${arch}*.appimage\"\n"),
        Kind::Static => out.push_str("glob   = \"TODO: asset glob, e.g. *${arch}*-linux-musl.tar.gz\"\n"),
    }
    out
}

/// Write the scaffold to `<root>/packages/<name>/pkg.toml`.
pub fn write(root: &Path, s: &Scaffold) -> Result<std::path::PathBuf, String> {
    let dir = root.join("packages").join(s.name);
    if dir.join("pkg.toml").exists() {
        return Err(format!("{} already exists", dir.join("pkg.toml").display()));
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("pkg.toml");
    fs::write(&path, render(s)).map_err(|e| e.to_string())?;
    Ok(path)
}
