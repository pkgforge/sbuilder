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
   --inplace             Replace the original file on success
   --help, -h            Show this help message

Arguments:
   FILE...               One or more package files to validate
```

## sbuilder

TODO
