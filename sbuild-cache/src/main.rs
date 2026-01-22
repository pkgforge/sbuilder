//! sbuild-cache CLI
//!
//! Command-line interface for managing the build cache.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use sbuild_cache::{BuildStatus, CacheDatabase, Result};

#[derive(Parser)]
#[command(name = "sbuild-cache")]
#[command(about = "Build cache management for SBUILD packages", long_about = None)]
#[command(version)]
struct Cli {
    /// Path to cache database
    #[arg(short, long, default_value = "build_cache.sdb")]
    cache: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
enum StatusFilter {
    Success,
    Failed,
    Pending,
    Skipped,
    Outdated,
    All,
}

#[derive(Clone, ValueEnum)]
enum ReportFormat {
    Markdown,
    Html,
    Json,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new cache database
    Init,

    /// Update a package's build status
    Update {
        /// Package identifier (pkg_id)
        #[arg(short, long)]
        package: String,

        /// Target architecture
        #[arg(short = 'H', long, default_value = "x86_64-Linux")]
        host: String,

        /// Package version
        #[arg(short, long)]
        version: String,

        /// Build status (success, failed, pending, skipped)
        #[arg(short, long)]
        status: String,

        /// Build ID
        #[arg(short, long)]
        build_id: Option<String>,

        /// GHCR tag
        #[arg(short, long)]
        tag: Option<String>,

        /// Recipe hash
        #[arg(long)]
        hash: Option<String>,
    },

    /// Mark a package as outdated
    MarkOutdated {
        /// Package identifier
        #[arg(short, long)]
        package: String,

        /// Target architecture
        #[arg(short = 'H', long, default_value = "x86_64-Linux")]
        host: String,

        /// Upstream version available
        #[arg(short, long)]
        upstream_version: String,
    },

    /// Show build statistics
    Stats {
        /// Target architecture
        #[arg(short = 'H', long, default_value = "x86_64-Linux")]
        host: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// List packages with optional filtering
    List {
        /// Target architecture
        #[arg(short = 'H', long, default_value = "x86_64-Linux")]
        host: String,

        /// Filter by status
        #[arg(short, long, value_enum, default_value = "all")]
        status: StatusFilter,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Limit number of results
        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// List packages needing rebuild
    NeedsRebuild {
        /// Target architecture
        #[arg(short = 'H', long, default_value = "x86_64-Linux")]
        host: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Generate a build status report
    Report {
        /// Target architecture
        #[arg(short = 'H', long, default_value = "x86_64-Linux")]
        host: String,

        /// Output format
        #[arg(short, long, value_enum, default_value = "markdown")]
        format: ReportFormat,

        /// Output file (stdout if not specified)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Include recent build history
        #[arg(long, default_value = "20")]
        history_limit: i64,
    },

    /// Show recent builds
    Recent {
        /// Target architecture
        #[arg(short = 'H', long, default_value = "x86_64-Linux")]
        host: String,

        /// Number of recent builds to show
        #[arg(short, long, default_value = "20")]
        limit: i64,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Prune old build history
    Prune {
        /// Keep last N builds per package
        #[arg(short, long, default_value = "10")]
        keep: i64,
    },

    /// Get package info
    Get {
        /// Package identifier
        #[arg(short, long)]
        package: String,

        /// Target architecture
        #[arg(short = 'H', long, default_value = "x86_64-Linux")]
        host: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Generate GitHub Actions summary (writes to $GITHUB_STEP_SUMMARY)
    GhSummary {
        /// Target architecture
        #[arg(short = 'H', long, default_value = "x86_64-Linux")]
        host: String,

        /// Title for the summary
        #[arg(short, long, default_value = "Build Status")]
        title: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            let db = CacheDatabase::open(&cli.cache)?;
            println!("Initialized cache database at {:?}", cli.cache);
            let stats = db.get_stats("x86_64-Linux")?;
            println!("Total packages: {}", stats.total_packages);
            Ok(())
        }

        Commands::Update {
            package,
            host,
            version,
            status,
            build_id,
            tag,
            hash,
        } => {
            let db = CacheDatabase::open(&cli.cache)?;

            let build_status = BuildStatus::from_str(&status)
                .ok_or_else(|| sbuild_cache::Error::InvalidStatus(status.clone()))?;

            // Ensure package exists
            let pkg_name = package.split('.').last().unwrap_or(&package);
            db.get_or_create_package(&package, pkg_name, &host)?;

            // Update build result
            db.update_build_result(
                &package,
                &host,
                &version,
                build_status,
                build_id.as_deref().unwrap_or("unknown"),
                tag.as_deref(),
                hash.as_deref(),
            )?;

            // Clear any failure records on success
            if build_status == BuildStatus::Success {
                db.clear_failure(&package, &host)?;
            }

            println!(
                "Updated {} on {} to version {} ({})",
                package, host, version, status
            );
            Ok(())
        }

        Commands::MarkOutdated {
            package,
            host,
            upstream_version,
        } => {
            let db = CacheDatabase::open(&cli.cache)?;
            db.mark_outdated(&package, &host, &upstream_version)?;
            println!(
                "Marked {} as outdated (upstream: {})",
                package, upstream_version
            );
            Ok(())
        }

        Commands::Stats { host, json } => {
            let db = CacheDatabase::open(&cli.cache)?;
            let stats = db.get_stats(&host)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("Build Statistics for {}", host);
                println!("========================");
                println!("Total packages:  {}", stats.total_packages);
                println!("Successful:      {}", stats.successful);
                println!("Failed:          {}", stats.failed);
                println!("Pending:         {}", stats.pending);
                println!("Outdated:        {}", stats.outdated);
            }
            Ok(())
        }

        Commands::NeedsRebuild { host, json } => {
            let db = CacheDatabase::open(&cli.cache)?;
            let packages = db.get_packages_needing_rebuild(&host)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&packages)?);
            } else {
                println!("Packages needing rebuild on {}:", host);
                println!();
                for pkg in &packages {
                    let status = pkg
                        .last_build_status
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "never built".to_string());
                    let version = pkg.current_version.as_deref().unwrap_or("unknown");
                    println!("  {} (v{}) - {}", pkg.pkg_name, version, status);
                    if pkg.is_outdated {
                        if let Some(ref upstream) = pkg.upstream_version {
                            println!("    -> upstream: {}", upstream);
                        }
                    }
                }
                println!();
                println!("Total: {} packages", packages.len());
            }
            Ok(())
        }

        Commands::Prune { keep } => {
            let db = CacheDatabase::open(&cli.cache)?;
            let deleted = db.prune_history(keep)?;
            println!("Pruned {} old build history entries", deleted);
            Ok(())
        }

        Commands::Get {
            package,
            host,
            json,
        } => {
            let db = CacheDatabase::open(&cli.cache)?;

            match db.get_package(&package, &host)? {
                Some(pkg) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&pkg)?);
                    } else {
                        println!("Package: {}", pkg.pkg_name);
                        println!("ID: {}", pkg.pkg_id);
                        println!("Host: {}", pkg.host_triplet);
                        println!(
                            "Version: {}",
                            pkg.current_version.as_deref().unwrap_or("unknown")
                        );
                        println!(
                            "Status: {}",
                            pkg.last_build_status
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "never built".to_string())
                        );
                        println!("Outdated: {}", pkg.is_outdated);
                        if let Some(ref hash) = pkg.recipe_hash {
                            println!("Recipe Hash: {}", hash);
                        }
                        if let Some(ref tag) = pkg.ghcr_tag {
                            println!("GHCR Tag: {}", tag);
                        }
                    }
                }
                None => {
                    eprintln!("Package not found: {} on {}", package, host);
                    std::process::exit(1);
                }
            }
            Ok(())
        }

        Commands::List {
            host,
            status,
            json,
            limit,
        } => {
            let db = CacheDatabase::open(&cli.cache)?;

            let (status_filter, include_outdated) = match status {
                StatusFilter::Success => (Some(BuildStatus::Success), false),
                StatusFilter::Failed => (Some(BuildStatus::Failed), false),
                StatusFilter::Pending => (Some(BuildStatus::Pending), false),
                StatusFilter::Skipped => (Some(BuildStatus::Skipped), false),
                StatusFilter::Outdated => (None, true),
                StatusFilter::All => (None, false),
            };

            let mut packages = db.list_packages(&host, status_filter, include_outdated)?;

            if let Some(limit) = limit {
                packages.truncate(limit);
            }

            if json {
                println!("{}", serde_json::to_string_pretty(&packages)?);
            } else {
                let status_icon = |s: Option<BuildStatus>| match s {
                    Some(BuildStatus::Success) => "✓",
                    Some(BuildStatus::Failed) => "✗",
                    Some(BuildStatus::Pending) => "○",
                    Some(BuildStatus::Skipped) => "⊘",
                    None => "?",
                };

                println!("Packages on {} ({} total):", host, packages.len());
                println!();
                println!(
                    "{:<3} {:<30} {:<15} {:<10}",
                    "", "Package", "Version", "Status"
                );
                println!("{}", "-".repeat(60));

                for pkg in &packages {
                    let version = pkg.current_version.as_deref().unwrap_or("-");
                    let icon = status_icon(pkg.last_build_status);
                    let outdated = if pkg.is_outdated { " [outdated]" } else { "" };
                    println!(
                        "{:<3} {:<30} {:<15} {:<10}{}",
                        icon,
                        pkg.pkg_name,
                        version,
                        pkg.last_build_status
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "never".to_string()),
                        outdated
                    );
                }
            }
            Ok(())
        }

        Commands::Recent { host, limit, json } => {
            let db = CacheDatabase::open(&cli.cache)?;
            let builds = db.get_recent_builds(&host, limit)?;

            if json {
                let output: Vec<_> = builds
                    .iter()
                    .map(|(pkg, hist)| {
                        serde_json::json!({
                            "package": pkg.pkg_name,
                            "version": hist.version,
                            "status": hist.build_status.to_string(),
                            "build_id": hist.build_id,
                            "build_date": hist.build_date.to_rfc3339(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("Recent builds on {}:", host);
                println!();
                println!(
                    "{:<3} {:<25} {:<12} {:<10} {:<20}",
                    "", "Package", "Version", "Status", "Date"
                );
                println!("{}", "-".repeat(75));

                for (pkg, hist) in &builds {
                    let icon = match hist.build_status {
                        BuildStatus::Success => "✓",
                        BuildStatus::Failed => "✗",
                        BuildStatus::Pending => "○",
                        BuildStatus::Skipped => "⊘",
                    };
                    let date = hist.build_date.format("%Y-%m-%d %H:%M");
                    println!(
                        "{:<3} {:<25} {:<12} {:<10} {:<20}",
                        icon, pkg.pkg_name, hist.version, hist.build_status, date
                    );
                }
            }
            Ok(())
        }

        Commands::Report {
            host,
            format,
            output,
            history_limit,
        } => {
            let db = CacheDatabase::open(&cli.cache)?;
            let stats = db.get_stats(&host)?;
            let failed = db.list_packages(&host, Some(BuildStatus::Failed), false)?;
            let outdated = db.list_packages(&host, None, true)?;
            let recent = db.get_recent_builds(&host, history_limit)?;

            let report = match format {
                ReportFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
                    "host": host,
                    "stats": stats,
                    "failed_packages": failed,
                    "outdated_packages": outdated,
                    "recent_builds": recent.iter().map(|(p, h)| {
                        serde_json::json!({
                            "package": p.pkg_name,
                            "version": h.version,
                            "status": h.build_status.to_string(),
                            "date": h.build_date.to_rfc3339(),
                        })
                    }).collect::<Vec<_>>(),
                }))?,
                ReportFormat::Markdown => {
                    generate_markdown_report(&host, &stats, &failed, &outdated, &recent)
                }
                ReportFormat::Html => {
                    generate_html_report(&host, &stats, &failed, &outdated, &recent)
                }
            };

            if let Some(path) = output {
                std::fs::write(&path, &report)?;
                println!("Report written to {:?}", path);
            } else {
                println!("{}", report);
            }
            Ok(())
        }

        Commands::GhSummary { host, title } => {
            let db = CacheDatabase::open(&cli.cache)?;
            let stats = db.get_stats(&host)?;
            let failed = db.list_packages(&host, Some(BuildStatus::Failed), false)?;

            let summary = generate_gh_summary(&title, &host, &stats, &failed);

            // Write to GITHUB_STEP_SUMMARY if available
            if let Ok(summary_path) = std::env::var("GITHUB_STEP_SUMMARY") {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&summary_path)?;
                writeln!(file, "{}", summary)?;
                println!("Summary written to GITHUB_STEP_SUMMARY");
            } else {
                // Just print to stdout if not in GitHub Actions
                println!("{}", summary);
            }
            Ok(())
        }
    }
}

fn generate_markdown_report(
    host: &str,
    stats: &sbuild_cache::BuildStats,
    failed: &[sbuild_cache::PackageRecord],
    outdated: &[sbuild_cache::PackageRecord],
    recent: &[(sbuild_cache::PackageRecord, sbuild_cache::BuildHistoryEntry)],
) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Build Report: {}\n\n", host));
    md.push_str(&format!(
        "Generated: {}\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    ));

    // Stats
    md.push_str("## Summary\n\n");
    md.push_str("| Metric | Count |\n");
    md.push_str("|--------|-------|\n");
    md.push_str(&format!("| Total Packages | {} |\n", stats.total_packages));
    md.push_str(&format!("| Successful | {} |\n", stats.successful));
    md.push_str(&format!("| Failed | {} |\n", stats.failed));
    md.push_str(&format!("| Pending | {} |\n", stats.pending));
    md.push_str(&format!("| Outdated | {} |\n\n", stats.outdated));

    // Success rate
    if stats.total_packages > 0 {
        let success_rate = (stats.successful as f64 / stats.total_packages as f64) * 100.0;
        md.push_str(&format!("**Success Rate: {:.1}%**\n\n", success_rate));
    }

    // Failed packages
    if !failed.is_empty() {
        md.push_str("## Failed Packages\n\n");
        md.push_str("| Package | Version | Last Build |\n");
        md.push_str("|---------|---------|------------|\n");
        for pkg in failed.iter().take(20) {
            let version = pkg.current_version.as_deref().unwrap_or("-");
            let date = pkg
                .last_build_date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "-".to_string());
            md.push_str(&format!("| {} | {} | {} |\n", pkg.pkg_name, version, date));
        }
        if failed.len() > 20 {
            md.push_str(&format!("\n*...and {} more*\n", failed.len() - 20));
        }
        md.push('\n');
    }

    // Outdated packages
    if !outdated.is_empty() {
        md.push_str("## Outdated Packages\n\n");
        md.push_str("| Package | Current | Upstream |\n");
        md.push_str("|---------|---------|----------|\n");
        for pkg in outdated.iter().take(20) {
            let current = pkg.current_version.as_deref().unwrap_or("-");
            let upstream = pkg.upstream_version.as_deref().unwrap_or("-");
            md.push_str(&format!(
                "| {} | {} | {} |\n",
                pkg.pkg_name, current, upstream
            ));
        }
        if outdated.len() > 20 {
            md.push_str(&format!("\n*...and {} more*\n", outdated.len() - 20));
        }
        md.push('\n');
    }

    // Recent builds
    if !recent.is_empty() {
        md.push_str("## Recent Builds\n\n");
        md.push_str("| Status | Package | Version | Date |\n");
        md.push_str("|--------|---------|---------|------|\n");
        for (pkg, hist) in recent {
            let icon = match hist.build_status {
                BuildStatus::Success => "✅",
                BuildStatus::Failed => "❌",
                BuildStatus::Pending => "⏳",
                BuildStatus::Skipped => "⏭️",
            };
            let date = hist.build_date.format("%Y-%m-%d %H:%M");
            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                icon, pkg.pkg_name, hist.version, date
            ));
        }
        md.push('\n');
    }

    md
}

fn generate_html_report(
    host: &str,
    stats: &sbuild_cache::BuildStats,
    failed: &[sbuild_cache::PackageRecord],
    outdated: &[sbuild_cache::PackageRecord],
    recent: &[(sbuild_cache::PackageRecord, sbuild_cache::BuildHistoryEntry)],
) -> String {
    let success_rate = if stats.total_packages > 0 {
        (stats.successful as f64 / stats.total_packages as f64) * 100.0
    } else {
        0.0
    };

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Build Report: {host}</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 1200px; margin: 0 auto; padding: 20px; }}
        h1 {{ color: #333; }}
        .stats {{ display: flex; gap: 20px; margin: 20px 0; flex-wrap: wrap; }}
        .stat {{ background: #f5f5f5; padding: 20px; border-radius: 8px; min-width: 120px; text-align: center; }}
        .stat.success {{ background: #d4edda; }}
        .stat.failed {{ background: #f8d7da; }}
        .stat.pending {{ background: #fff3cd; }}
        .stat-value {{ font-size: 2em; font-weight: bold; }}
        .stat-label {{ color: #666; }}
        table {{ width: 100%; border-collapse: collapse; margin: 20px 0; }}
        th, td {{ padding: 10px; text-align: left; border-bottom: 1px solid #ddd; }}
        th {{ background: #f5f5f5; }}
        .success {{ color: #28a745; }}
        .failed {{ color: #dc3545; }}
        .pending {{ color: #ffc107; }}
        .progress {{ width: 100%; height: 20px; background: #e9ecef; border-radius: 4px; overflow: hidden; }}
        .progress-bar {{ height: 100%; background: #28a745; }}
    </style>
</head>
<body>
    <h1>Build Report: {host}</h1>
    <p>Generated: {timestamp}</p>

    <div class="stats">
        <div class="stat"><div class="stat-value">{total}</div><div class="stat-label">Total</div></div>
        <div class="stat success"><div class="stat-value">{success}</div><div class="stat-label">Success</div></div>
        <div class="stat failed"><div class="stat-value">{fail}</div><div class="stat-label">Failed</div></div>
        <div class="stat pending"><div class="stat-value">{pending}</div><div class="stat-label">Pending</div></div>
    </div>

    <h3>Success Rate: {success_rate:.1}%</h3>
    <div class="progress"><div class="progress-bar" style="width: {success_rate:.1}%"></div></div>

    {failed_section}
    {outdated_section}
    {recent_section}
</body>
</html>"#,
        host = host,
        timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        total = stats.total_packages,
        success = stats.successful,
        fail = stats.failed,
        pending = stats.pending,
        success_rate = success_rate,
        failed_section = if !failed.is_empty() {
            let rows: String = failed
                .iter()
                .take(20)
                .map(|pkg| {
                    format!(
                        "<tr><td>{}</td><td>{}</td></tr>",
                        pkg.pkg_name,
                        pkg.current_version.as_deref().unwrap_or("-")
                    )
                })
                .collect();
            format!("<h2>Failed Packages ({} total)</h2><table><tr><th>Package</th><th>Version</th></tr>{}</table>",
                failed.len(), rows)
        } else {
            String::new()
        },
        outdated_section = if !outdated.is_empty() {
            let rows: String = outdated
                .iter()
                .take(20)
                .map(|pkg| {
                    format!(
                        "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                        pkg.pkg_name,
                        pkg.current_version.as_deref().unwrap_or("-"),
                        pkg.upstream_version.as_deref().unwrap_or("-")
                    )
                })
                .collect();
            format!("<h2>Outdated Packages ({} total)</h2><table><tr><th>Package</th><th>Current</th><th>Upstream</th></tr>{}</table>",
                outdated.len(), rows)
        } else {
            String::new()
        },
        recent_section = if !recent.is_empty() {
            let rows: String = recent
                .iter()
                .map(|(pkg, hist)| {
                    let class = match hist.build_status {
                        BuildStatus::Success => "success",
                        BuildStatus::Failed => "failed",
                        _ => "pending",
                    };
                    format!(
                        "<tr><td class=\"{}\">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                        class,
                        match hist.build_status {
                            BuildStatus::Success => "✅",
                            BuildStatus::Failed => "❌",
                            _ => "⏳",
                        },
                        pkg.pkg_name,
                        hist.version,
                        hist.build_date.format("%Y-%m-%d %H:%M")
                    )
                })
                .collect();
            format!("<h2>Recent Builds</h2><table><tr><th>Status</th><th>Package</th><th>Version</th><th>Date</th></tr>{}</table>", rows)
        } else {
            String::new()
        },
    )
}

fn generate_gh_summary(
    title: &str,
    host: &str,
    stats: &sbuild_cache::BuildStats,
    failed: &[sbuild_cache::PackageRecord],
) -> String {
    let success_rate = if stats.total_packages > 0 {
        (stats.successful as f64 / stats.total_packages as f64) * 100.0
    } else {
        0.0
    };

    let mut summary = String::new();

    summary.push_str(&format!("## {} ({})\n\n", title, host));

    // Stats badges
    summary.push_str(&format!(
        "| ✅ Success | ❌ Failed | ⏳ Pending | 📊 Total | 🎯 Rate |\n"
    ));
    summary.push_str("|---|---|---|---|---|\n");
    summary.push_str(&format!(
        "| {} | {} | {} | {} | {:.1}% |\n\n",
        stats.successful, stats.failed, stats.pending, stats.total_packages, success_rate
    ));

    // Failed packages
    if !failed.is_empty() {
        summary.push_str("### ❌ Failed Packages\n\n");
        summary.push_str("<details><summary>Show failed packages</summary>\n\n");
        summary.push_str("| Package | Version |\n");
        summary.push_str("|---------|--------|\n");
        for pkg in failed.iter().take(50) {
            let version = pkg.current_version.as_deref().unwrap_or("-");
            summary.push_str(&format!("| {} | {} |\n", pkg.pkg_name, version));
        }
        if failed.len() > 50 {
            summary.push_str(&format!("\n*...and {} more*\n", failed.len() - 50));
        }
        summary.push_str("\n</details>\n");
    }

    summary
}
