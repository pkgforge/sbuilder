# Sbuilder

A repo containing the linter and builder for SBUILD packages.

## sbuild-linter
The linter validates the provided `SBUILD` package recipe, performs checks and generates the validated recipe for the builder to work with.

```sh
Usage: sbuild-linter [OPTIONS] [FILES]

Options:
--pkgver              Enable pkgver mode
--no-shellcheck       Disable shellcheck
--help, -h            Show this help message

Files:
Specify one or more files to process.
```

## sbuilder

TODO
