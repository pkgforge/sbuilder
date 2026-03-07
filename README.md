# Sbuilder

Toolchain for building, linting, metadata generation, and cache management of SBUILD packages.

All functionality is provided through a single `sbuild` binary.

```
Usage: sbuild <COMMAND>

Commands:
  build  Build packages from SBUILD recipes
  info   Get information about an SBUILD recipe
  cache  Build cache management for SBUILD packages
  lint   Linter for SBUILD package files
  meta   Metadata generator for SBUILD packages
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## sbuild build

Build packages from SBUILD recipes.

```
Usage: sbuild build [OPTIONS] <RECIPES>...

Arguments:
  <RECIPES>...  SBUILD recipe files or URLs to build

Options:
  -o, --outdir <OUTDIR>              Output directory for build artifacts
  -k, --keep                         Keep temporary build directory after completion
      --timeout <TIMEOUT>            Build timeout in seconds [default: 3600]
      --timeout-linter <TIMEOUT>     Linter timeout in seconds [default: 30]
      --log-level <LOG_LEVEL>        Log level for build output [default: info] [possible values: info, verbose, debug]
      --ci                           CI mode - output GitHub Actions environment variables
      --force                        Force rebuild even if package exists
      --skip-existing                Skip build if output already exists
      --github-token <GITHUB_TOKEN>  GitHub token for authenticated requests [env: GITHUB_TOKEN]
      --ghcr-token <GHCR_TOKEN>      GHCR token for pushing packages [env: GHCR_TOKEN]
      --ghcr-repo <GHCR_REPO>        GHCR repository base (e.g., pkgforge/bincache)
      --push                         Push packages to GHCR after build
      --dry-run                      Simulate GHCR push without actual push
      --sign                         Sign packages with minisign
      --minisign-key <MINISIGN_KEY>  Minisign private key (or path to key file) [env: MINISIGN_KEY]
      --minisign-password <PASSWORD> Minisign private key password [env: MINISIGN_PASSWORD]
      --checksums                    Generate checksums for built artifacts
      --cache <CACHE>                Path to build cache database
  -h, --help                         Print help
```

## sbuild info

Get information about an SBUILD recipe.

```
Usage: sbuild info [OPTIONS] <RECIPE>

Arguments:
  <RECIPE>  SBUILD recipe file or URL

Options:
      --check-host <CHECK_HOST>  Check if recipe supports this host (e.g., x86_64-linux)
      --format <FORMAT>          Output format [default: text] [possible values: text, json]
      --field <FIELD>            Output specific field (pkg, pkg_id, version, hosts, etc.)
  -h, --help                     Print help
```

## sbuild lint

Linter for SBUILD package files. Validates SBUILD recipe files, performs checks and generates the validated recipe for the builder.

```
Usage: sbuild lint [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Files to lint

Options:
  -P, --pkgver               Enable pkgver mode
      --no-shellcheck        Disable shellcheck
  -p, --parallel <PARALLEL>  Run N jobs in parallel [default: 4]
  -i, --inplace              Replace the original file on success
      --success <SUCCESS>    File to store successful packages list
      --fail <FAIL>          File to store failed packages list
      --timeout <TIMEOUT>    Timeout duration in seconds [default: 30]
  -h, --help                 Print help
```

## sbuild meta

Metadata generator for SBUILD packages.

```
Usage: sbuild meta <COMMAND>

Commands:
  generate        Generate metadata for packages
  should-rebuild  Check if a recipe should be rebuilt
  check-updates   Check for upstream updates
  inspect         Inspect recipe and generate metadata
  hash            Compute hash of a recipe
  fetch-manifest  Fetch and display manifest for a package
  help            Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### sbuild meta generate

```
Usage: sbuild meta generate [OPTIONS] --arch <ARCH>

Options:
  -a, --arch <ARCH>                  Target architecture (x86_64-linux, aarch64-linux, riscv64-linux)
  -r, --recipes <RECIPES>...         Recipe directories to scan
  -o, --output <OUTPUT>              Output directory for JSON files
  -c, --cache <CACHE>                Historical cache database (optional)
  -p, --parallel <PARALLEL>          Number of parallel workers [default: 4]
      --github-token <GITHUB_TOKEN>  GitHub token for registry access [env: GITHUB_TOKEN]
      --ghcr-owner <GHCR_OWNER>      GHCR owner/organization [default: pkgforge]
  -h, --help                         Print help
```

### sbuild meta should-rebuild

```
Usage: sbuild meta should-rebuild [OPTIONS] --recipe <RECIPE>

Options:
  -r, --recipe <RECIPE>  Path to SBUILD recipe
  -c, --cache <CACHE>    Path to cache database
  -f, --force            Force rebuild regardless of status
  -h, --help             Print help
```

### sbuild meta check-updates

```
Usage: sbuild meta check-updates [OPTIONS] --output <OUTPUT>

Options:
  -r, --recipes <RECIPES>...  Recipe directories to scan
  -c, --cache <CACHE>         Path to cache database
  -o, --output <OUTPUT>       Output JSON file with outdated packages
  -p, --parallel <PARALLEL>   Number of parallel workers [default: 10]
      --timeout <TIMEOUT>     Timeout for pkgver script execution (in seconds) [default: 30]
  -h, --help                  Print help
```

### sbuild meta inspect

```
Usage: sbuild meta inspect [OPTIONS] <RECIPE>

Arguments:
  <RECIPE>  Path to SBUILD recipe

Options:
  -a, --arch <ARCH>              Target architecture [default: x86_64-linux]
      --ghcr-owner <GHCR_OWNER>  GHCR owner/organization [default: pkgforge]
      --live                     Fetch live manifest data from GHCR
  -h, --help                     Print help
```

### sbuild meta hash

```
Usage: sbuild meta hash [OPTIONS] <RECIPE>

Arguments:
  <RECIPE>  Path to SBUILD recipe

Options:
      --exclude-version  Exclude version field from hash
  -h, --help             Print help
```

### sbuild meta fetch-manifest

```
Usage: sbuild meta fetch-manifest [OPTIONS] --repository <REPOSITORY>

Options:
  -r, --repository <REPOSITORY>      Package repository (e.g., pkgforge/bincache/bat)
  -t, --tag <TAG>                    Tag to fetch (optional, uses latest arch-specific if not provided)
  -a, --arch <ARCH>                  Target architecture [default: x86_64-linux]
      --github-token <GITHUB_TOKEN>  GitHub token for registry access [env: GITHUB_TOKEN]
  -h, --help                         Print help
```

## sbuild cache

Build cache management for SBUILD packages. Supports both SQLite and MongoDB backends.

```
Usage: sbuild cache [OPTIONS] <COMMAND>

Commands:
  init           Initialize a new cache database
  update         Update a package's build status
  mark-outdated  Mark a package as outdated
  stats          Show build statistics
  list           List packages with optional filtering
  report         Generate a build status report
  recent         Show recent builds
  prune          Prune old build history
  get            Get package info
  gh-summary     Generate GitHub Actions summary (writes to $GITHUB_STEP_SUMMARY)
  export         Export MongoDB cache to SQLite file
  help           Print this message or the help of the given subcommand(s)

Options:
  -c, --cache <CACHE>  Path to cache database [default: build_cache.sdb]
  -h, --help           Print help
```

### sbuild cache init

```
Usage: sbuild cache init
```

### sbuild cache update

```
Usage: sbuild cache update [OPTIONS] --package <PACKAGE> --version <VERSION> --status <STATUS>

Options:
  -p, --package <PACKAGE>    Package identifier (pkg_id)
  -H, --host <HOST>          Target architecture [default: x86_64-linux]
  -v, --version <VERSION>    Package version
  -s, --status <STATUS>      Build status (success, failed, pending, skipped)
  -b, --build-id <BUILD_ID>  Build ID
  -t, --tag <TAG>            GHCR tag
      --hash <HASH>          Recipe hash
  -h, --help                 Print help
```

### sbuild cache mark-outdated

```
Usage: sbuild cache mark-outdated [OPTIONS] --package <PACKAGE> --upstream-version <UPSTREAM_VERSION>

Options:
  -p, --package <PACKAGE>                    Package identifier
  -H, --host <HOST>                          Target architecture [default: x86_64-linux]
  -u, --upstream-version <UPSTREAM_VERSION>  Upstream version available
  -h, --help                                 Print help
```

### sbuild cache stats

```
Usage: sbuild cache stats [OPTIONS]

Options:
  -H, --host <HOST>  Target architecture [default: x86_64-linux]
      --json         Output as JSON
  -h, --help         Print help
```

### sbuild cache list

```
Usage: sbuild cache list [OPTIONS]

Options:
  -H, --host <HOST>      Target architecture [default: x86_64-linux]
  -s, --status <STATUS>  Filter by status [default: all] [possible values: success, failed, pending, skipped, outdated, all]
      --json             Output as JSON
  -l, --limit <LIMIT>    Limit number of results
  -h, --help             Print help
```

### sbuild cache needs-rebuild

```
Usage: sbuild cache needs-rebuild [OPTIONS]

Options:
  -H, --host <HOST>  Target architecture [default: x86_64-linux]
      --json         Output as JSON
  -h, --help         Print help
```

### sbuild cache report

```
Usage: sbuild cache report [OPTIONS]

Options:
  -H, --host <HOST>                    Target architecture [default: x86_64-linux]
  -f, --format <FORMAT>                Output format [default: markdown] [possible values: markdown, html, json]
  -o, --output <OUTPUT>                Output file (stdout if not specified)
      --history-limit <HISTORY_LIMIT>  Include recent build history [default: 20]
  -h, --help                           Print help
```

### sbuild cache recent

```
Usage: sbuild cache recent [OPTIONS]

Options:
  -H, --host <HOST>    Target architecture [default: x86_64-linux]
  -l, --limit <LIMIT>  Number of recent builds to show [default: 20]
      --json           Output as JSON
  -h, --help           Print help
```

### sbuild cache prune

```
Usage: sbuild cache prune [OPTIONS]

Options:
  -k, --keep <KEEP>  Keep last N builds per package [default: 10]
  -h, --help         Print help
```

### sbuild cache get

```
Usage: sbuild cache get [OPTIONS] --package <PACKAGE>

Options:
  -p, --package <PACKAGE>  Package identifier
  -H, --host <HOST>        Target architecture [default: x86_64-linux]
      --json               Output as JSON
  -h, --help               Print help
```

### sbuild cache gh-summary

```
Usage: sbuild cache gh-summary [OPTIONS]

Options:
  -H, --host <HOST>    Target architecture [default: x86_64-linux]
  -t, --title <TITLE>  Title for the summary [default: "Build Status"]
  -h, --help           Print help
```

### sbuild cache export

```
Usage: sbuild cache export [OPTIONS]

Options:
  -o, --output <OUTPUT>  Output SQLite file [default: build_cache.sdb]
  -h, --help             Print help
```
