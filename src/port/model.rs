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

/// What a package takes out of its artifact.
///
/// Most entries are just a path and where it lands, so they read better as
/// `"from" = "to"`. The long form exists for what that cannot say: aliases,
/// and an artifact that is the file itself.
///
/// A host may also be named, for a package whose artifacts are not laid out
/// alike everywhere: one served by a build of our own and the rest taken from
/// upstream unpack differently, and no single map describes both. Naming a
/// host replaces the shared list for that host rather than adding to it, so
/// the common case stays written once.
#[derive(Debug, Default)]
pub struct InstallSpec {
    shared: Vec<InstallEntry>,
    per_host: BTreeMap<String, Vec<InstallEntry>>,
}

/// How the list may be written, before the two kinds of key are told apart.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawInstall {
    Long(Vec<InstallEntry>),
    Table(BTreeMap<String, RawValue>),
}

/// A value in that table: a destination for a shared entry, or one host's own
/// list. Which it is follows from its shape, so no marker key is needed.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawValue {
    To(String),
    Entries(Vec<InstallEntry>),
    Map(BTreeMap<String, String>),
}

fn short_entries(map: BTreeMap<String, String>) -> Vec<InstallEntry> {
    map.into_iter()
        .map(|(from, to)| InstallEntry {
            from: Some(from),
            to: Some(to),
            symlink_as: Vec::new(),
        })
        .collect()
}

impl<'de> Deserialize<'de> for InstallSpec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match RawInstall::deserialize(d)? {
            RawInstall::Long(entries) => Self {
                shared: entries,
                per_host: BTreeMap::new(),
            },
            RawInstall::Table(table) => {
                let mut shared = BTreeMap::new();
                let mut per_host = BTreeMap::new();
                for (key, value) in table {
                    match value {
                        RawValue::To(to) => {
                            shared.insert(key, to);
                        }
                        RawValue::Entries(entries) => {
                            per_host.insert(key, entries);
                        }
                        RawValue::Map(map) => {
                            per_host.insert(key, short_entries(map));
                        }
                    }
                }
                Self {
                    shared: short_entries(shared),
                    per_host,
                }
            }
        })
    }
}

impl InstallSpec {
    /// The entries for one host: its own list if it names one, else the shared.
    pub fn entries(&self, host: &str) -> Vec<InstallEntry> {
        self.per_host.get(host).unwrap_or(&self.shared).clone()
    }

    /// Every entry any host installs, for checks that hold regardless of host.
    pub fn all_entries(&self) -> Vec<InstallEntry> {
        self.shared
            .iter()
            .chain(self.per_host.values().flatten())
            .cloned()
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.shared.is_empty() && self.per_host.values().all(Vec::is_empty)
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
    fn a_shared_list_applies_to_every_host() {
        let s = spec("[install]\n\"btm\" = \"bin/btm\"\n");
        for host in ["x86_64-linux", "riscv64-linux"] {
            assert_eq!(s.entries(host).len(), 1);
        }
    }

    #[test]
    fn naming_a_host_replaces_the_shared_list_for_it_alone() {
        let s = spec(
            "[install]\n\"pfx/nu\" = \"bin/nu\"\n\
             [install.riscv64-linux]\n\"nu\" = \"bin/nu\"\n\"LICENSE\" = \"LICENSE\"\n",
        );
        // Hosts that say nothing keep the shared list, so it is written once.
        for host in ["x86_64-linux", "aarch64-linux"] {
            assert_eq!(s.entries(host)[0].from.as_deref(), Some("pfx/nu"));
        }
        assert_eq!(s.entries("riscv64-linux").len(), 2);
        assert_eq!(s.all_entries().len(), 3);
        assert!(!s.is_empty());
    }

    #[test]
    fn a_host_may_install_where_the_shared_list_is_empty() {
        // Upstream ships a bare binary; only our build unpacks into paths.
        let s = spec("[install.riscv64-linux]\n\"d\" = \"bin/d\"\n");
        assert!(s.entries("x86_64-linux").is_empty());
        assert_eq!(s.entries("riscv64-linux").len(), 1);
    }

    #[test]
    fn the_long_form_still_carries_aliases() {
        let s = spec("install = [{ from = \"a\", to = \"bin/a\", symlink_as = [\"b\"] }]\n");
        assert_eq!(s.entries("x86_64-linux")[0].symlink_as, vec!["b"]);
    }
}
