use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use sbuild_cache::{BuildStatus, CacheDatabase, MongoDatabase, Result};

#[derive(Parser)]
#[command(about = "Build cache management for SBUILD packages")]
pub struct CacheArgs {
    #[arg(short, long, default_value = "build_cache.sdb")]
    cache: PathBuf,

    #[command(subcommand)]
    command: CacheCommands,
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
enum CacheCommands {
    Init,

    Update {
        #[arg(short, long)]
        package: String,

        #[arg(short = 'H', long, default_value = "x86_64-linux")]
        host: String,

        #[arg(short, long)]
        version: String,

        #[arg(short, long)]
        status: String,

        #[arg(short, long)]
        build_id: Option<String>,

        #[arg(short, long)]
        tag: Option<String>,

        #[arg(long)]
        hash: Option<String>,
    },

    MarkOutdated {
        #[arg(short, long)]
        package: String,

        #[arg(short = 'H', long, default_value = "x86_64-linux")]
        host: String,

        #[arg(short, long)]
        upstream_version: String,
    },

    Stats {
        #[arg(short = 'H', long, default_value = "x86_64-linux")]
        host: String,

        #[arg(long)]
        json: bool,
    },

    List {
        #[arg(short = 'H', long, default_value = "x86_64-linux")]
        host: String,

        #[arg(short, long, value_enum, default_value = "all")]
        status: StatusFilter,

        #[arg(long)]
        json: bool,

        #[arg(short, long)]
        limit: Option<usize>,
    },

    NeedsRebuild {
        #[arg(short = 'H', long, default_value = "x86_64-linux")]
        host: String,

        #[arg(long)]
        json: bool,
    },

    Report {
        #[arg(short = 'H', long, default_value = "x86_64-linux")]
        host: String,

        #[arg(short, long, value_enum, default_value = "markdown")]
        format: ReportFormat,

        #[arg(short, long)]
        output: Option<PathBuf>,

        #[arg(long, default_value = "20")]
        history_limit: i64,
    },

    Recent {
        #[arg(short = 'H', long, default_value = "x86_64-linux")]
        host: String,

        #[arg(short, long, default_value = "20")]
        limit: i64,

        #[arg(long)]
        json: bool,
    },

    Prune {
        #[arg(short, long, default_value = "10")]
        keep: i64,
    },

    Get {
        #[arg(short, long)]
        package: String,

        #[arg(short = 'H', long, default_value = "x86_64-linux")]
        host: String,

        #[arg(long)]
        json: bool,
    },

    GhSummary {
        #[arg(short = 'H', long, default_value = "x86_64-linux")]
        host: String,

        #[arg(short, long, default_value = "Build Status")]
        title: String,
    },

    /// Export MongoDB cache to SQLite file
    Export {
        #[arg(short, long, default_value = "build_cache.sdb")]
        output: PathBuf,
    },
}

/// Get the cache URI from environment variable
fn get_cache_uri() -> Option<String> {
    std::env::var("SBUILD_CACHE_URI")
        .ok()
        .filter(|s| !s.is_empty())
}

pub async fn run(args: CacheArgs) -> Result<()> {
    match args.command {
        CacheCommands::Init => {
            if let Some(uri) = get_cache_uri() {
                let _db = MongoDatabase::connect(&uri).await?;
                println!("Connected to MongoDB cache");
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                println!("Initialized cache database at {:?}", args.cache);
                let stats = db.get_stats("x86_64-linux")?;
                println!("Total packages: {}", stats.total_packages);
            }
            Ok(())
        }
        CacheCommands::Update {
            package,
            host,
            version,
            status,
            build_id,
            tag,
            hash,
        } => {
            let build_status = BuildStatus::from_str(&status)
                .ok_or_else(|| sbuild_cache::Error::InvalidStatus(status.clone()))?;
            let pkg_name = package.rsplit('.').next().unwrap_or(&package);

            if let Some(uri) = get_cache_uri() {
                let db = MongoDatabase::connect(&uri).await?;
                db.get_or_create_package(&package, pkg_name, &host).await?;
                db.update_build_result(
                    &package,
                    &host,
                    &version,
                    build_status,
                    build_id.as_deref(),
                    tag.as_deref(),
                    hash.as_deref(),
                    None,
                    0,
                    None,
                    None,
                    None,
                )
                .await?;
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                db.get_or_create_package(&package, pkg_name, &host)?;
                db.update_build_result(
                    &package,
                    &host,
                    &version,
                    build_status,
                    build_id.as_deref(),
                    tag.as_deref(),
                    hash.as_deref(),
                    None,
                    0,
                )?;
                if build_status == BuildStatus::Success {
                    db.clear_failure(&package, &host)?;
                }
            }

            println!(
                "Updated {} on {} to version {} ({})",
                package, host, version, status
            );
            Ok(())
        }
        CacheCommands::MarkOutdated {
            package,
            host,
            upstream_version,
        } => {
            if let Some(uri) = get_cache_uri() {
                let db = MongoDatabase::connect(&uri).await?;
                db.mark_outdated(&package, &host, &upstream_version).await?;
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                db.mark_outdated(&package, &host, &upstream_version)?;
            }
            println!(
                "Marked {} as outdated (upstream: {})",
                package, upstream_version
            );
            Ok(())
        }
        CacheCommands::Stats { host, json } => {
            let stats = if let Some(uri) = get_cache_uri() {
                let db = MongoDatabase::connect(&uri).await?;
                db.get_stats(&host).await?
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                db.get_stats(&host)?
            };

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
        CacheCommands::NeedsRebuild { host, json } => {
            let packages = if let Some(uri) = get_cache_uri() {
                let db = MongoDatabase::connect(&uri).await?;
                db.get_packages_needing_rebuild(&host).await?
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                db.get_packages_needing_rebuild(&host)?
            };

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
        CacheCommands::Prune { keep } => {
            // Prune only applies to SQLite (MongoDB handles this via $slice)
            let db = CacheDatabase::open(&args.cache)?;
            let deleted = db.prune_history(keep)?;
            println!("Pruned {} old build history entries", deleted);
            Ok(())
        }
        CacheCommands::Get {
            package,
            host,
            json,
        } => {
            let packages = if let Some(uri) = get_cache_uri() {
                let db = MongoDatabase::connect(&uri).await?;
                if let Some(pkg) = db.get_package(&package, &host).await? {
                    vec![pkg]
                } else {
                    db.find_packages_by_name(&package, &host).await?
                }
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                if let Some(pkg) = db.get_package(&package, &host)? {
                    vec![pkg]
                } else {
                    db.find_packages_by_name(&package, &host)?
                }
            };

            if packages.is_empty() {
                eprintln!("Package not found: {} on {}", package, host);
                std::process::exit(1);
            }

            if json {
                if packages.len() == 1 {
                    println!("{}", serde_json::to_string_pretty(&packages[0])?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&packages)?);
                }
            } else {
                for (i, pkg) in packages.iter().enumerate() {
                    if i > 0 {
                        println!("{}", "-".repeat(40));
                    }
                    println!("Package: {}", pkg.pkg_name);
                    println!("ID: {}", pkg.pkg_id);
                    println!("Host: {}", pkg.host_triplet);
                    println!(
                        "Version: {}",
                        pkg.current_version.as_deref().unwrap_or("unknown")
                    );
                    if let Some(ref base) = pkg.base_version {
                        println!("Base Version: {}", base);
                        println!("Revision: {}", pkg.revision);
                    }
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
            Ok(())
        }
        CacheCommands::List {
            host,
            status,
            json,
            limit,
        } => {
            let (status_filter, include_outdated) = match status {
                StatusFilter::Success => (Some(BuildStatus::Success), false),
                StatusFilter::Failed => (Some(BuildStatus::Failed), false),
                StatusFilter::Pending => (Some(BuildStatus::Pending), false),
                StatusFilter::Skipped => (Some(BuildStatus::Skipped), false),
                StatusFilter::Outdated => (None, true),
                StatusFilter::All => (None, false),
            };

            let mut packages = if let Some(uri) = get_cache_uri() {
                let db = MongoDatabase::connect(&uri).await?;
                db.list_packages(&host, status_filter, include_outdated)
                    .await?
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                db.list_packages(&host, status_filter, include_outdated)?
            };

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
        CacheCommands::Recent { host, limit, json } => {
            let builds = if let Some(uri) = get_cache_uri() {
                let db = MongoDatabase::connect(&uri).await?;
                db.get_recent_builds(&host, limit).await?
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                db.get_recent_builds(&host, limit)?
            };

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
        CacheCommands::Report {
            host,
            format,
            output,
            history_limit,
        } => {
            let (stats, failed, outdated, recent) = if let Some(uri) = get_cache_uri() {
                let db = MongoDatabase::connect(&uri).await?;
                let stats = db.get_stats(&host).await?;
                let failed = db
                    .list_packages(&host, Some(BuildStatus::Failed), false)
                    .await?;
                let outdated = db.list_packages(&host, None, true).await?;
                let recent = db.get_recent_builds(&host, history_limit).await?;
                (stats, failed, outdated, recent)
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                let stats = db.get_stats(&host)?;
                let failed = db.list_packages(&host, Some(BuildStatus::Failed), false)?;
                let outdated = db.list_packages(&host, None, true)?;
                let recent = db.get_recent_builds(&host, history_limit)?;
                (stats, failed, outdated, recent)
            };

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
        CacheCommands::GhSummary { host, title } => {
            let (stats, failed) = if let Some(uri) = get_cache_uri() {
                let db = MongoDatabase::connect(&uri).await?;
                let stats = db.get_stats(&host).await?;
                let failed = db
                    .list_packages(&host, Some(BuildStatus::Failed), false)
                    .await?;
                (stats, failed)
            } else {
                let db = CacheDatabase::open(&args.cache)?;
                let stats = db.get_stats(&host)?;
                let failed = db.list_packages(&host, Some(BuildStatus::Failed), false)?;
                (stats, failed)
            };

            let success_rate = if stats.total_packages > 0 {
                (stats.successful as f64 / stats.total_packages as f64) * 100.0
            } else {
                0.0
            };

            let mut summary = String::new();
            summary.push_str(&format!("## {} ({})\n\n", title, host));
            summary.push_str("| ✅ Success | ❌ Failed | ⏳ Pending | 📊 Total | 🎯 Rate |\n");
            summary.push_str("|---|---|---|---|---|\n");
            summary.push_str(&format!(
                "| {} | {} | {} | {} | {:.1}% |\n\n",
                stats.successful, stats.failed, stats.pending, stats.total_packages, success_rate
            ));

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

            if let Ok(summary_path) = std::env::var("GITHUB_STEP_SUMMARY") {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&summary_path)?;
                writeln!(file, "{}", summary)?;
                println!("Summary written to GITHUB_STEP_SUMMARY");
            } else {
                println!("{}", summary);
            }
            Ok(())
        }
        CacheCommands::Export { output } => {
            let uri = get_cache_uri().ok_or_else(|| {
                sbuild_cache::Error::Other(
                    "SBUILD_CACHE_URI environment variable is required for export".to_string(),
                )
            })?;
            let mongo = MongoDatabase::connect(&uri).await?;
            sbuild_cache::export::export_to_sqlite(&mongo, &output).await?;
            println!("Exported MongoDB cache to {:?}", output);
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

    md.push_str("## Summary\n\n");
    md.push_str("| Metric | Count |\n");
    md.push_str("|--------|-------|\n");
    md.push_str(&format!("| Total Packages | {} |\n", stats.total_packages));
    md.push_str(&format!("| Successful | {} |\n", stats.successful));
    md.push_str(&format!("| Failed | {} |\n", stats.failed));
    md.push_str(&format!("| Pending | {} |\n", stats.pending));
    md.push_str(&format!("| Outdated | {} |\n\n", stats.outdated));

    if stats.total_packages > 0 {
        let success_rate = (stats.successful as f64 / stats.total_packages as f64) * 100.0;
        md.push_str(&format!("**Success Rate: {:.1}%**\n\n", success_rate));
    }

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
    _failed: &[sbuild_cache::PackageRecord],
    _outdated: &[sbuild_cache::PackageRecord],
    _recent: &[(sbuild_cache::PackageRecord, sbuild_cache::BuildHistoryEntry)],
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
        table {{ width: 100%; border-collapse: collapse; margin: 20px 0; }}
        th, td {{ padding: 10px; text-align: left; border-bottom: 1px solid #ddd; }}
        th {{ background: #f5f5f5; }}
    </style>
</head>
<body>
    <h1>Build Report: {host}</h1>
    <p>Generated: {timestamp}</p>
    <div class="stats">
        <div class="stat"><div class="stat-value">{total}</div><div class="stat-label">Total</div></div>
        <div class="stat"><div class="stat-value">{success}</div><div class="stat-label">Success</div></div>
        <div class="stat"><div class="stat-value">{fail}</div><div class="stat-label">Failed</div></div>
        <div class="stat"><div class="stat-value">{pending}</div><div class="stat-label">Pending</div></div>
    </div>
    <h3>Success Rate: {success_rate:.1}%</h3>
</body>
</html>"#,
        host = host,
        timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        total = stats.total_packages,
        success = stats.successful,
        fail = stats.failed,
        pending = stats.pending,
        success_rate = success_rate,
    )
}
