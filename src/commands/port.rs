use std::path::PathBuf;

use clap::Subcommand;
use colored::Colorize;

use futures::StreamExt;
use sbuild::port::{audit, hashfill, meta, new, resolve, tree, validate};

#[derive(Subcommand)]
pub enum PortCommands {
    /// Check the tree; every pinned URL must have a hash beside it
    Validate {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// Scaffold a new package
    New {
        /// Package name
        name: String,
        /// Upstream repository, e.g. pkgforge-dev/Foo-AppImage
        repo: String,
        #[arg(long, default_value = "appimage")]
        r#type: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        maintainer: Option<String>,
        /// Upstream tags carry a build suffix like 1.2.3@2026-01-01_1234
        #[arg(long)]
        tag_suffix_strip: bool,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Pin the current upstream version of every package
    Resolve {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Restrict to these packages
        packages: Vec<String>,
        #[arg(long, env = "GITHUB_TOKEN", default_value = "")]
        github_token: String,
    },
    /// Hash any pinned artifact the forge API had no digest for
    Hashfill {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Concurrent downloads
        #[arg(long, default_value = "12")]
        jobs: usize,
    },
    /// Download each archive and check every install path resolves in it
    Audit {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Restrict to these packages
        packages: Vec<String>,
        /// Hosts to audit; repeat the flag for more than one
        #[arg(long, default_values_t = [String::from("x86_64-linux")])]
        host: Vec<String>,
        #[arg(long, default_value = "10")]
        jobs: usize,
    },
    /// Generate the soar metadata index for one host
    Meta {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(short, long)]
        arch: String,
        #[arg(short, long)]
        output: PathBuf,
    },
}

/// Borrow the token `gh` is already holding.
///
/// Unauthenticated the forge allows sixty requests an hour, which a tree this
/// size exhausts in seconds. Anyone resolving packages almost certainly has
/// `gh` logged in, so asking it beats making them export a variable.
fn token_from_gh() -> Option<String> {
    let out = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let token = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!token.is_empty()).then_some(token)
}

pub async fn run(command: PortCommands) -> Result<(), String> {
    match command {
        PortCommands::Validate { root } => {
            let r = validate::run(&root);
            println!("checked {} pinned artifacts across {} packages", r.checked, r.packages);
            for w in &r.warnings {
                println!("  {}  {w}", "warn".yellow());
            }
            for e in &r.errors {
                println!("  {} {e}", "ERROR".red());
            }
            println!("\n{} errors, {} warnings", r.errors.len(), r.warnings.len());
            if r.errors.is_empty() {
                Ok(())
            } else {
                Err(format!("{} validation errors", r.errors.len()))
            }
        }
        PortCommands::New { name, repo, r#type, description, maintainer, tag_suffix_strip, root } => {
            let scaffold = new::Scaffold {
                name: &name,
                repo: &repo,
                kind: new::Kind::parse(&r#type)?,
                description: description.as_deref(),
                maintainer: maintainer.as_deref(),
                tag_suffix_strip,
            };
            let path = new::write(&root, &scaffold)?;
            println!("wrote {}", path.display());
            println!("next: sbuild port resolve {} {name}", root.display());
            Ok(())
        }
        PortCommands::Resolve { root, packages, github_token } => {
            let github_token = if github_token.is_empty() {
                match token_from_gh() {
                    Some(t) => {
                        println!("using the token from gh");
                        t
                    }
                    None => {
                        eprintln!(
                            "  {} no GITHUB_TOKEN and gh is not logged in; \
                             the forge allows 60 requests an hour without one",
                            "warn".yellow()
                        );
                        github_token
                    }
                }
            } else {
                github_token
            };
            let client = reqwest::Client::builder()
                .user_agent("sbuild-port")
                .build()
                .map_err(|e| e.to_string())?;
            let (pkgs, errs) = tree::load(&root);
            for e in &errs {
                eprintln!("  {} {e}", "warn".yellow());
            }
            let (mut ok, mut partial, mut failed, mut skipped) = (0, 0, 0, 0);
            let mut checked = 0usize;
            let total = pkgs.len();
            for p in &pkgs {
                // Files are named after the directory, but a caller naming a
                // package on the command line means the package: fd lives in
                // packages/fd-find, and asking for `fd` should find it.
                let name = p.pkg.family().to_string();
                if !packages.is_empty()
                    && !packages.contains(&name)
                    && !packages.contains(&p.pkg.pkg.name)
                {
                    continue;
                }
                if p.pkg.pkg.disabled || p.pkg.update.is_none() || p.pkg.source.is_none() {
                    skipped += 1;
                    continue;
                }
                checked += 1;
                match resolve::resolve(&client, &p.pkg, &github_token).await {
                    Ok(mut r) => {
                        let dest = p.dir.join(format!("{}-{}.toml", name, r.version));
                        // Only a new pin is worth a line. Reporting every
                        // package would bury the handful that moved.
                        if !dest.exists() {
                            println!("  {} {name} {}", "pinned".green(), r.version);
                        }
                        if let Ok(prev) = std::fs::read_to_string(&dest) {
                            if let Ok(v) = toml::from_str(&prev) {
                                resolve::carry_forward(&mut r, &v);
                            }
                        }
                        std::fs::write(&dest, resolve::render(&r)).map_err(|e| e.to_string())?;
                        if r.missing.is_empty() {
                            ok += 1;
                        } else {
                            partial += 1;
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        eprintln!("  {} {name}: {e}", "FAIL".red());
                        // One exhausted quota fails every remaining package
                        // for the same reason; repeating it is just noise.
                        if e.starts_with(resolve::RATE_LIMITED) {
                            eprintln!(
                                "  {} stopping: set GITHUB_TOKEN or run `gh auth login`",
                                "warn".yellow()
                            );
                            break;
                        }
                    }
                }
                if checked % 50 == 0 {
                    eprintln!("  ... {checked}/{total}");
                }
            }
            println!("fully pinned {ok} | partial {partial} | skipped {skipped} | failed {failed}");
            Ok(())
        }
        PortCommands::Hashfill { root, jobs } => {
            let client = reqwest::Client::builder()
                .user_agent("sbuild-port")
                .build()
                .map_err(|e| e.to_string())?;
            let gaps = hashfill::gaps(&root);
            if gaps.is_empty() {
                println!("no artifact hashes missing");
            }
            let total = gaps.len();
            if total > 0 {
                println!("hashing {total} artifacts with {jobs} concurrent downloads ...");
            }
            let mut by_file: std::collections::BTreeMap<PathBuf, std::collections::BTreeMap<String, (String, String, u64)>> =
                Default::default();
            let mut failed = 0;
            let mut done = 0;

            // Artifacts are streamed and discarded, so concurrency costs
            // bandwidth but not memory or disk.
            let mut stream = futures::stream::iter(gaps.iter().map(|g| {
                let client = client.clone();
                async move { (g, hashfill::digests(&client, &g.url).await) }
            }))
            .buffer_unordered(jobs);

            // Merge as each artifact lands rather than at the end. A full
            // run is tens of gigabytes, and hashfill skips anything that
            // already has blake3, so writing incrementally makes it resumable.
            while let Some((g, res)) = stream.next().await {
                done += 1;
                match res {
                    Ok((b3, sha, n)) => {
                        let mut one = std::collections::BTreeMap::new();
                        one.insert(g.host.clone(), (b3, sha, n));
                        if let Err(e) = hashfill::merge(&g.path, &one) {
                            failed += 1;
                            eprintln!("  {} {}: {e}", "WRITE".red(), g.path.display());
                        } else {
                            by_file.insert(g.path.clone(), Default::default());
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        eprintln!("  {} {}: {e}", "FAIL".red(), g.url);
                    }
                }
                if done % 25 == 0 {
                    eprintln!("  ... {done}/{total} ({failed} failed)");
                }
            }
            if total > 0 {
                println!("filled {} hashes across {} files ({failed} failed)",
                         total - failed, by_file.len());
            }

            // Side files are pinned per version too: most point at a branch,
            // so their content can change with no version bump.
            let egaps = hashfill::extra_gaps(&root);
            if !egaps.is_empty() {
                println!("hashing {} side files ...", egaps.len());
                let mut per_file: std::collections::BTreeMap<PathBuf, Vec<(String, String, Option<String>, String, String)>> =
                    Default::default();
                let mut efailed = 0;
                let mut estream = futures::stream::iter(egaps.iter().map(|g| {
                    let client = client.clone();
                    async move {
                        let res = match &g.local {
                            Some(p) => hashfill::digests_local(p),
                            None => hashfill::digests(&client, &g.url).await,
                        };
                        (g, res)
                    }
                }))
                .buffer_unordered(jobs);
                let mut edone = 0;
                while let Some((g, res)) = estream.next().await {
                    edone += 1;
                    match res {
                        Ok((b3, sha, _)) => per_file
                            .entry(g.path.clone())
                            .or_default()
                            .push((g.url.clone(), g.to.clone(), g.host.clone(), b3, sha)),
                        Err(e) => {
                            efailed += 1;
                            eprintln!("  {} {}: {e}", "FAIL".red(), g.url);
                        }
                    }
                    if edone % 50 == 0 {
                        eprintln!("  ... {edone}/{}", egaps.len());
                    }
                }
                for (path, items) in &per_file {
                    hashfill::merge_extras(path, items)?;
                }
                println!("pinned {} side files across {} versions ({efailed} failed)",
                         egaps.len() - efailed, per_file.len());
            }
            Ok(())
        }
        PortCommands::Audit { root, packages, host, jobs } => {
            let findings = audit::run(&root, &host, &packages, jobs).await?;
            let mut ok = 0;
            let mut unlistable = Vec::new();
            let mut problems = Vec::new();
            for f in &findings {
                match &f.outcome {
                    audit::Outcome::Ok => ok += 1,
                    audit::Outcome::Unlistable => unlistable.push(f),
                    _ => problems.push(f),
                }
            }
            println!("\n{ok}/{} verified against real archive contents", findings.len());
            if !unlistable.is_empty() {
                // A bare binary, or one compressed on its own, has no members
                // to list. The artifact is the file the package installs, so
                // there are no interior paths that could be wrong.
                println!(
                    "\n{} not archives, nothing to verify inside:",
                    unlistable.len()
                );
                for f in &unlistable {
                    println!("  {} ({})", f.package, f.host);
                }
            }
            for f in &problems {
                match &f.outcome {
                    audit::Outcome::Missing(m) => {
                        println!("  {} {} ({})", "MISSING".red(), f.package, f.host);
                        for x in m {
                            println!("          no archive member matches: {x}");
                        }
                    }
                    audit::Outcome::Failed(e) => {
                        println!("  {} {} ({}): {e}", "FAILED".red(), f.package, f.host)
                    }
                    _ => {}
                }
            }
            if problems.is_empty() {
                Ok(())
            } else {
                Err(format!("{} packages have unresolvable install paths", problems.len()))
            }
        }
        PortCommands::Meta { root, arch, output } => {
            let (entries, errors) = meta::generate(&root, &arch);
            for e in &errors {
                eprintln!("  {} {e}", "warn".yellow());
            }
            let count = entries.len();
            let json = serde_json::to_string_pretty(&meta::Index::new(entries))
                .map_err(|e| e.to_string())?;
            std::fs::write(&output, json).map_err(|e| e.to_string())?;
            println!("wrote {count} entries -> {}", output.display());
            Ok(())
        }
    }
}
