# Sbuilder

A repo containing the linter and builder for SBUILD packages.

## sbuild-linter

The linter validates the provided `SBUILD` package recipe, performs checks and generates the validated recipe for the builder to work with.

```sh
Usage: sbuild-linter [OPTIONS] [FILES]

A linter for SBUILD package files.

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

## sbuilder

```sh
Usage: sbuild [OPTIONS] [FILES]

A builder for SBUILD package files.

Options:
   --help, -h                   Show this help message
   --log-level                  Log level for build script: info/1 (default), verbose/2, debug/3
   --keep, -k                   Whether to keep sbuild temp directory
   --outdir, -o <PATH>          Directory to store the build files in
   --timeout-linter <DURATION>  Timeout duration after which the linter exists

Arguments:
   FILE...               One or more package files to build
```
