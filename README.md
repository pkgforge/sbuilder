# sbuild

Tooling for the [soarpkgs](https://github.com/pkgforge/soarpkgs) package
format. One binary, six commands, no state of its own.

soarpkgs is declarative: a package is a recipe saying where upstream publishes
its artifact, and a version file pinning the exact URL and hash of one release.
Nothing in it executes. `sbuild` is what turns the first into the second, and
what turns the whole tree into an index a client can read.

```
recipe        pkg.toml            what upstream publishes, as a template
   |  resolve
version file  <name>-<version>.toml   one release, pinned by URL and hash
   |  meta
index         metadata-<host>.json    what soar reads
```

The format itself is specified in
[soarpkgs/docs/FORMAT.md](https://github.com/pkgforge/soarpkgs/blob/main/docs/FORMAT.md),
not here, so that the spec sits with the tree it describes.

## Install

Statically linked, no dependencies:

```sh
curl -fsSL "https://github.com/pkgforge/sbuilder/releases/download/nightly/sbuild-$(uname -m)-linux" -o sbuild
chmod +x sbuild
```

Built for `x86_64`, `aarch64` and `riscv64`, each with a `.b3sum` beside it.
Or `cargo install --git https://github.com/pkgforge/sbuilder`.

## Commands

Every command takes the tree root as its first argument, defaulting to `.`.

### `resolve`

Ask each upstream what its current version is, and write the version file that
pins it: the URL per host, plus whatever digest the forge already reports.

```sh
sbuild resolve                      # every package
sbuild resolve . ripgrep fzf        # only these
```

Needs `GITHUB_TOKEN` for anything but a handful of packages, since it is one
API call per package. Re-resolving an unchanged package rewrites nothing.

### `hashfill`

Most forges report a sha256 for an asset; none report blake3, which is what
soar verifies against. This downloads whatever is pinned without a hash beside
it, streaming the body so memory stays flat regardless of artifact size.

```sh
sbuild hashfill --jobs 12
```

Run it after `resolve`, before committing. A pinned URL with no hash is the
one state `validate` refuses.

### `validate`

Check the tree. Every pinned URL has a hash, every install target stays inside
the package directory, every recipe parses and says what it must.

```sh
sbuild validate
```

This is the gate in CI, and the only one that has to pass before a merge.

### `audit`

Download each archive and check that every path the recipe installs actually
resolves inside it. `validate` cannot see this: it reads the tree, and whether
`bin/foo` exists in a tarball is a fact about the tarball.

```sh
sbuild audit --host x86_64-linux --host aarch64-linux
```

Slow and network-bound by design, so it runs on its own rather than per merge.

### `meta`

Generate the index for one host, out of the tree and nothing else. No network,
no build state: same tree in, same index out, which is why it needs no runner
of the architecture it generates for.

```sh
sbuild meta --arch riscv64-linux --output metadata-riscv64-linux.json
```

soar reads this after `soar json2db` turns it into SQLite.

### `new`

Scaffold a recipe, so a new package starts from something that parses.

```sh
sbuild new ripgrep BurntSushi/ripgrep --type static
```

## Determinism

Both writers order every host-keyed table by the `[url]` table, so a file only
changes when what it pins changes. Anything else would churn a diff on every
run and bury the one line somebody needs to review, which is the whole point
of pinning in a file a human reads.
