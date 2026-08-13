//! Data model for the declarative port format.
//!
//! A package is described by two kinds of file:
//!
//! * `pkg.toml` holds identity, metadata and the update policy, stable across
//!   releases.
//! * `<name>-<version>.toml` is one per pinned version, holding the resolved
//!   per-host URL, hash and size.
//!
//! Nothing here is executable. A client can resolve a package by parsing
//! alone, which is what allows the hash to live in the repository rather than
//! being measured after the fact.

use indexmap::IndexMap;
use std::collections::BTreeMap;

use serde::Deserialize;

/// A parsed `pkg.toml`.
#[derive(Debug, Deserialize)]
pub struct PkgToml {
    pub pkg: Pkg,
    #[serde(default)]
    pub host: Host,
    /// Maps a host architecture onto the name upstream uses for it.
    #[serde(default)]
    pub arch: BTreeMap<String, String>,
    pub update: Option<Update>,
    pub source: Option<Source>,
    #[serde(default)]
    pub extra: Vec<Extra>,
}

#[derive(Debug, Deserialize)]
pub struct Pkg {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub description: Option<String>,
    /// Directory name, which is not always `name` (packages/fd-find holds fd).
    pub family: Option<String>,
    /// Release channel; `stable` unless stated.
    pub channel: Option<String>,
    #[serde(default)]
    pub homepage: Vec<String>,
    /// Upstream repository. Derived from the update policy when omitted.
    #[serde(default)]
    pub src: Vec<String>,
    #[serde(default)]
    pub license: Vec<String>,
    #[serde(default)]
    pub maintainer: Vec<String>,
    #[serde(default)]
    pub category: Vec<String>,
    #[serde(default)]
    pub repology: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
    /// Only notes that cannot be derived from the structured fields.
    #[serde(default)]
    pub note: Vec<String>,
    #[serde(default = "yes")]
    pub portable: bool,
    #[serde(rename = "portable-reason")]
    pub portable_reason: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(rename = "disabled-reason")]
    pub disabled_reason: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
pub struct Host {
    #[serde(default)]
    pub supported: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Update {
    pub strategy: String,
    pub repo: String,
    #[serde(rename = "strip-prefix")]
    pub strip_prefix: Option<String>,
    #[serde(rename = "tag-suffix")]
    pub tag_suffix: Option<String>,
    /// Only consider releases whose tag starts with this. Needed when one
    /// repository publishes releases for several packages.
    #[serde(rename = "tag-prefix")]
    pub tag_prefix: Option<String>,
    pub pattern: Option<String>,
}

/// How the bot locates the artifact for a release. Never used at install time.
#[derive(Debug, Deserialize)]
pub struct Source {
    pub url: Option<UrlSpec>,
    pub github: Option<String>,
    pub tag: Option<String>,
    pub glob: Option<String>,
    #[serde(default)]
    pub r#match: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// What the package takes out of its artifact. Empty means the whole
    /// artifact is the package.
    #[serde(default)]
    pub install: InstallSpec,
}

/// The install list, in either form a recipe may write it.
///
/// Most entries are just a path and where it lands, so they read better as
/// `"from" = "to"`. The long form exists for what that cannot say: aliases,
/// and an artifact that is the file itself.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum InstallSpec {
    Short(BTreeMap<String, String>),
    Long(Vec<InstallEntry>),
    /// Keyed by host, for an artifact whose layout is not the same everywhere.
    /// A package taken from upstream for most hosts and built for one unpacks
    /// differently on that one, and no single map describes both.
    PerHost(BTreeMap<String, InstallSpec>),
}

impl Default for InstallSpec {
    fn default() -> Self {
        Self::Long(Vec::new())
    }
}

impl InstallSpec {
    /// The entries for one host. A map that is not per-host applies to all.
    pub fn entries(&self, host: &str) -> Vec<InstallEntry> {
        match self {
            Self::PerHost(by_host) => {
                by_host.get(host).map(|s| s.entries(host)).unwrap_or_default()
            }
            _ => self.own_entries(),
        }
    }

    /// Every entry any host installs, for checks that hold regardless of host.
    pub fn all_entries(&self) -> Vec<InstallEntry> {
        match self {
            Self::PerHost(by_host) => by_host.values().flat_map(|s| s.all_entries()).collect(),
            _ => self.own_entries(),
        }
    }

    /// The entries as this map itself declares them, ignoring any host split.
    fn own_entries(&self) -> Vec<InstallEntry> {
        match self {
            Self::Long(entries) => entries
                .iter()
                .map(|e| InstallEntry {
                    from: e.from.clone(),
                    to: e.to.clone(),
                    symlink_as: e.symlink_as.clone(),
                })
                .collect(),
            Self::Short(map) => map
                .iter()
                .map(|(from, to)| InstallEntry {
                    from: Some(from.clone()),
                    to: Some(to.clone()),
                    symlink_as: Vec::new(),
                })
                .collect(),
            Self::PerHost(_) => Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Long(e) => e.is_empty(),
            Self::Short(m) => m.is_empty(),
            Self::PerHost(m) => m.values().all(Self::is_empty),
        }
    }
}

/// One file the package installs.
#[derive(Debug, Clone, Deserialize)]
pub struct InstallEntry {
    /// Path inside the artifact. Absent when the artifact is the file itself,
    /// as a bare binary is.
    pub from: Option<String>,
    /// Where it lands inside the package directory. Defaults to `bin/` plus
    /// the file's own name.
    pub to: Option<String>,
    /// Extra names for the same file, created beside `to`. Where they end up
    /// on the system follows from the directory, the same way `to` does: an
    /// alias beside `bin/dunstctl` is another command, one beside a man page
    /// is another man page.
    #[serde(default)]
    pub symlink_as: Vec<String>,
}

impl InstallEntry {
    /// The install path, resolved against the default.
    pub fn target(&self) -> String {
        if let Some(to) = &self.to {
            return to.clone();
        }
        let name = self
            .from
            .as_deref()
            .unwrap_or_default()
            .rsplit('/')
            .next()
            .unwrap_or_default();
        format!("bin/{name}")
    }

    /// Paths, relative to the package directory, that also resolve to this
    /// file. Each is a sibling of `to`, so it inherits the same meaning.
    pub fn aliases(&self) -> Vec<String> {
        let target = self.target();
        let dir = target.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        self.symlink_as
            .iter()
            .map(|name| {
                if dir.is_empty() {
                    name.clone()
                } else {
                    format!("{dir}/{name}")
                }
            })
            .collect()
    }
}

/// A single template, or one URL per host when upstream filenames differ
/// irreconcilably between architectures.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum UrlSpec {
    Template(String),
    PerHost(BTreeMap<String, String>),
}

/// A side file the artifact does not ship, typically a licence.
///
/// In `pkg.toml` this is a template the bot resolves. Nearly all of these
/// point at a branch rather than a tag, so the content can change without any
/// version bump; the resolved copy and its hash therefore live in the version
/// file and are refreshed whenever a version is pinned.
#[derive(Debug, Deserialize)]
pub struct Extra {
    /// Fetched from upstream.
    pub url: Option<String>,
    /// Or taken from this repository's `licenses/` directory by SPDX id.
    ///
    /// Only valid for licences whose text is verbatim by requirement (the GPL
    /// family). MIT, BSD and friends embed a per-project copyright line, so
    /// only the project's own file will do.
    pub license: Option<String>,
    pub to: String,
    /// Whether to pin this file's content. Defaults to true.
    ///
    /// Set false for a file that legitimately changes without a version bump,
    /// such as a licence served from a branch. A pinned hash would turn an
    /// upstream copyright-year edit into a failed download for everyone
    /// installing that version, and a licence is documentation rather than
    /// something that runs.
    pub verify: Option<bool>,
}

impl Extra {
    /// Whether this file's content should be pinned.
    pub fn verify(&self) -> bool {
        self.verify.unwrap_or(true)
    }
}

/// A resolved side file: the URL actually fetched and what it hashed to.
#[derive(Debug, Clone, Deserialize)]
pub struct PinnedExtra {
    pub url: String,
    pub to: String,
    /// Set when the file differs per host, as an upstream's per-arch binary
    /// does. Absent means it applies to every host, which is the case for a
    /// licence.
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub blake3: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

/// A parsed `<name>-<version>.toml`.
#[derive(Debug, Deserialize)]
pub struct VersionToml {
    pub version: String,
    /// When upstream published this release. Recorded because a version built
    /// from a commit hash carries no order of its own, so a client comparing
    /// two snapshots has nothing else to go on.
    #[serde(default)]
    pub date: Option<String>,
    /// Insertion-ordered, so the file's own host order is preserved rather
    /// than resorted on every rewrite.
    #[serde(default)]
    pub url: IndexMap<String, String>,
    /// blake3 digests per host. This is what soar verifies downloads
    /// against, and it can only be obtained by fetching the artifact.
    #[serde(default)]
    pub blake3: IndexMap<String, String>,
    /// sha256 digests per host. Reported by forge APIs without downloading,
    /// so it doubles as a cross-check against a release's checksums file.
    #[serde(default)]
    pub sha256: IndexMap<String, String>,
    #[serde(default)]
    pub size: IndexMap<String, u64>,
    /// Any `pkg.toml` field may be overridden per version; these are the ones
    /// that realistically change between releases.
    #[serde(default)]
    pub note: Option<Vec<String>>,
    #[serde(default)]
    pub provides: Option<Vec<String>>,
    /// Side files resolved and hashed for this version.
    #[serde(default)]
    pub extra: Vec<PinnedExtra>,
}

impl PkgToml {
    /// Directory name, falling back to the package name.
    pub fn family(&self) -> &str {
        self.pkg.family.as_deref().unwrap_or(&self.pkg.name)
    }

    pub fn channel(&self) -> &str {
        self.pkg.channel.as_deref().unwrap_or("stable")
    }

    /// Upstream repository, derived from the update policy when not stated.
    pub fn src_urls(&self) -> Vec<String> {
        if !self.pkg.src.is_empty() {
            return self.pkg.src.clone();
        }
        match &self.update {
            Some(u) if u.strategy.starts_with("github") => {
                vec![format!("https://github.com/{}", u.repo)]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(toml: &str) -> InstallSpec {
        #[derive(Deserialize)]
        struct Wrapper {
            install: InstallSpec,
        }
        toml::from_str::<Wrapper>(toml).unwrap().install
    }

    #[test]
    fn a_shared_map_applies_to_every_host() {
        let s = spec("[install]\n\"btm\" = \"bin/btm\"\n");
        for host in ["x86_64-linux", "riscv64-linux"] {
            assert_eq!(s.entries(host).len(), 1);
        }
    }

    #[test]
    fn a_per_host_map_answers_for_the_host_asked_about() {
        let s = spec(
            "[install.x86_64-linux]\n\"pfx/nu\" = \"bin/nu\"\n\
             [install.riscv64-linux]\n\"nu\" = \"bin/nu\"\n\"LICENSE\" = \"LICENSE\"\n",
        );
        assert_eq!(s.entries("x86_64-linux")[0].from.as_deref(), Some("pfx/nu"));
        assert_eq!(s.entries("riscv64-linux").len(), 2);
        // A host the recipe says nothing about installs nothing, rather than
        // silently taking another host's layout.
        assert!(s.entries("aarch64-linux").is_empty());
        // Host-independent checks still see every host's entries.
        assert_eq!(s.all_entries().len(), 3);
        assert!(!s.is_empty());
    }

    #[test]
    fn a_host_with_no_entries_means_the_artifact_is_the_package() {
        let s = spec("[install.x86_64-linux]\n[install.riscv64-linux]\n\"d\" = \"bin/d\"\n");
        assert!(s.entries("x86_64-linux").is_empty());
        assert_eq!(s.entries("riscv64-linux").len(), 1);
    }
}
