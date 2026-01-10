# Sbuilder

Toolchain for building, linting, metadata generation, and cache management of SBUILD packages.

## sbuild

Build packages from SBUILD recipes.

```sh
Usage: sbuild <COMMAND>

Commands:
  build  Build packages from SBUILD recipes
  info   Get information about an SBUILD recipe
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

### sbuild build

```sh
Usage: sbuild build [OPTIONS] <RECIPES>...

Arguments:
  <RECIPES>...  SBUILD recipe files or URLs to build

Options:
  -o, --outdir <OUTDIR>                Output directory for build artifacts
  -k, --keep                           Keep temporary build directory after completion
      --timeout <TIMEOUT>              Build timeout in seconds [default: 3600]
      --timeout-linter <TIMEOUT>       Linter timeout in seconds [default: 30]
      --log-level <LOG_LEVEL>          Log level for build output [default: info] [possible values: info, verbose, debug]
      --ci                             CI mode - output GitHub Actions environment variables
      --force                          Force rebuild even if package exists
      --github-token <GITHUB_TOKEN>    GitHub token for authenticated requests [env: GITHUB_TOKEN]
      --ghcr-token <GHCR_TOKEN>        GHCR token for pushing packages [env: GHCR_TOKEN]
      --ghcr-repo <GHCR_REPO>          GHCR repository base (e.g., pkgforge/bincache)
      --push                           Push packages to GHCR after build
      --sign                           Sign packages with minisign
      --minisign-key <MINISIGN_KEY>    Minisign private key (or path to key file) [env: MINISIGN_KEY]
      --minisign-password <PASSWORD>   Minisign private key password [env: MINISIGN_PASSWORD]
      --checksums <CHECKSUMS>          Generate checksums for built artifacts [default: true]
  -h, --help                           Print help
```

### sbuild info

```sh
Usage: sbuild info [OPTIONS] <RECIPE>

Arguments:
  <RECIPE>  SBUILD recipe file or URL

Options:
      --check-host <CHECK_HOST>  Check if recipe supports this host (e.g., x86_64-Linux)
      --format <FORMAT>          Output format [default: text] [possible values: text, json]
      --field <FIELD>            Output specific field (pkg, pkg_id, version, hosts, etc.)
  -h, --help                     Print help
```

## sbuild-linter

A linter for SBUILD package files. Validates the provided `SBUILD` package recipe, performs checks and generates the validated recipe for the builder to work with.

```sh
Usage: sbuild-linter [OPTIONS] [FILES]

Options:
   --pkgver, -p          Enable pkgver mode
   --no-shellcheck       Disable shellcheck
   --parallel <N>        Run N jobs in parallel (default: 4)
   --inplace, -i         Replace the original file on success
   --success <PATH>      File to store successful packages list
   --fail <PATH>         File to store failed packages list
   --timeout <DURATION>  Timeout duration after which the pkgver check exits
   --help, -h            Show this help message

Arguments:
   FILE...               One or more package files to validate
```

## sbuild-meta

Metadata generator for SBUILD packages.

```sh
Usage: sbuild-meta [OPTIONS] <COMMAND>

Commands:
  generate        Generate metadata for packages
  should-rebuild  Check if a recipe should be rebuilt
  check-updates   Check for upstream updates
  hash            Compute hash of a recipe
  fetch-manifest  Fetch and display manifest for a package
  help            Print this message or the help of the given subcommand(s)

Options:
      --log-level <LOG_LEVEL>  Log level (error, warn, info, debug, trace) [default: info]
  -h, --help                   Print help
  -V, --version                Print version
```

### sbuild-meta generate

```sh
Usage: sbuild-meta generate [OPTIONS] --arch <ARCH> --recipes <RECIPES>... --output <OUTPUT>

Options:
  -a, --arch <ARCH>              Target architecture (x86_64-Linux, aarch64-Linux, riscv64-Linux)
  -r, --recipes <RECIPES>...     Recipe directories to scan
  -o, --output <OUTPUT>          Output directory for JSON files (creates {cache_type}/{arch}.json)
      --cache-type <CACHE_TYPE>  Cache type to generate (bincache, pkgcache, or all) [default: all]
  -c, --cache <CACHE>            Historical cache database (optional)
  -p, --parallel <PARALLEL>      Number of parallel workers [default: 4]
      --github-token <TOKEN>     GitHub token for registry access [env: GITHUB_TOKEN]
      --ghcr-owner <GHCR_OWNER>  GHCR owner/organization [default: pkgforge]
  -h, --help                     Print help
```

### sbuild-meta should-rebuild

```sh
Usage: sbuild-meta should-rebuild [OPTIONS] --recipe <RECIPE>

Options:
  -r, --recipe <RECIPE>  Path to SBUILD recipe
  -c, --cache <CACHE>    Path to cache database
  -f, --force            Force rebuild regardless of status
  -h, --help             Print help
```

### sbuild-meta check-updates

```sh
Usage: sbuild-meta check-updates [OPTIONS] --recipes <RECIPES>... --output <OUTPUT>

Options:
  -r, --recipes <RECIPES>...  Recipe directories to scan
  -c, --cache <CACHE>         Path to cache database
  -o, --output <OUTPUT>       Output JSON file with outdated packages
  -p, --parallel <PARALLEL>   Number of parallel workers [default: 10]
      --timeout <TIMEOUT>     Timeout for pkgver script execution (in seconds) [default: 30]
  -h, --help                  Print help
```

### sbuild-meta hash

```sh
Usage: sbuild-meta hash [OPTIONS] <RECIPE>

Arguments:
  <RECIPE>  Path to SBUILD recipe

Options:
      --exclude-version  Exclude version field from hash
  -h, --help             Print help
```

### sbuild-meta fetch-manifest

```sh
Usage: sbuild-meta fetch-manifest [OPTIONS] --repository <REPOSITORY>

Options:
  -r, --repository <REPOSITORY>  Package repository (e.g., pkgforge/bincache/bat)
  -t, --tag <TAG>                Tag to fetch (optional, uses latest arch-specific if not provided)
  -a, --arch <ARCH>              Target architecture [default: x86_64-Linux]
      --github-token <TOKEN>     GitHub token for registry access [env: GITHUB_TOKEN]
  -h, --help                     Print help
```

## sbuild-cache

Build cache management for SBUILD packages.

```sh
Usage: sbuild-cache [OPTIONS] <COMMAND>

Commands:
  init           Initialize a new cache database
  update         Update a package's build status
  mark-outdated  Mark a package as outdated
  stats          Show build statistics
  list           List packages with optional filtering
  needs-rebuild  List packages needing rebuild
  report         Generate a build status report
  recent         Show recent builds
  prune          Prune old build history
  get            Get package info
  gh-summary     Generate GitHub Actions summary (writes to $GITHUB_STEP_SUMMARY)
  help           Print this message or the help of the given subcommand(s)

Options:
  -c, --cache <CACHE>  Path to cache database [default: build_cache.sdb]
  -h, --help           Print help
  -V, --version        Print version
```

### sbuild-cache init

```sh
Usage: sbuild-cache init
```

### sbuild-cache update

```sh
Usage: sbuild-cache update [OPTIONS] --package <PACKAGE> --version <VERSION> --status <STATUS>

Options:
  -p, --package <PACKAGE>    Package identifier (pkg_id)
  -H, --host <HOST>          Target architecture [default: x86_64-Linux]
  -v, --version <VERSION>    Package version
  -s, --status <STATUS>      Build status (success, failed, pending, skipped)
  -b, --build-id <BUILD_ID>  Build ID
  -t, --tag <TAG>            GHCR tag
      --hash <HASH>          Recipe hash
  -h, --help                 Print help
```

### sbuild-cache mark-outdated

```sh
Usage: sbuild-cache mark-outdated [OPTIONS] --package <PACKAGE> --upstream-version <UPSTREAM_VERSION>

Options:
  -p, --package <PACKAGE>                    Package identifier
  -H, --host <HOST>                          Target architecture [default: x86_64-Linux]
  -u, --upstream-version <UPSTREAM_VERSION>  Upstream version available
  -h, --help                                 Print help
```

### sbuild-cache stats

```sh
Usage: sbuild-cache stats [OPTIONS]

Options:
  -H, --host <HOST>  Target architecture [default: x86_64-Linux]
      --json         Output as JSON
  -h, --help         Print help
```

### sbuild-cache list

```sh
Usage: sbuild-cache list [OPTIONS]

Options:
  -H, --host <HOST>      Target architecture [default: x86_64-Linux]
  -s, --status <STATUS>  Filter by status [default: all] [possible values: success, failed, pending, skipped, outdated, all]
      --json             Output as JSON
  -l, --limit <LIMIT>    Limit number of results
  -h, --help             Print help
```

### sbuild-cache needs-rebuild

```sh
Usage: sbuild-cache needs-rebuild [OPTIONS]

Options:
  -H, --host <HOST>  Target architecture [default: x86_64-Linux]
      --json         Output as JSON
  -h, --help         Print help
```

### sbuild-cache report

```sh
Usage: sbuild-cache report [OPTIONS]

Options:
  -H, --host <HOST>                  Target architecture [default: x86_64-Linux]
  -f, --format <FORMAT>              Output format [default: markdown] [possible values: markdown, html, json]
  -o, --output <OUTPUT>              Output file (stdout if not specified)
      --history-limit <LIMIT>        Include recent build history [default: 20]
  -h, --help                         Print help
```

### sbuild-cache recent

```sh
Usage: sbuild-cache recent [OPTIONS]

Options:
  -H, --host <HOST>    Target architecture [default: x86_64-Linux]
  -l, --limit <LIMIT>  Number of recent builds to show [default: 20]
      --json           Output as JSON
  -h, --help           Print help
```

### sbuild-cache prune

```sh
Usage: sbuild-cache prune [OPTIONS]

Options:
  -k, --keep <KEEP>  Keep last N builds per package [default: 10]
  -h, --help         Print help
```

### sbuild-cache get

```sh
Usage: sbuild-cache get [OPTIONS] --package <PACKAGE>

Options:
  -p, --package <PACKAGE>  Package identifier
  -H, --host <HOST>        Target architecture [default: x86_64-Linux]
      --json               Output as JSON
  -h, --help               Print help
```

### sbuild-cache gh-summary

```sh
Usage: sbuild-cache gh-summary [OPTIONS]

Options:
  -H, --host <HOST>    Target architecture [default: x86_64-Linux]
  -t, --title <TITLE>  Title for the summary [default: "Build Status"]
  -h, --help           Print help
```
