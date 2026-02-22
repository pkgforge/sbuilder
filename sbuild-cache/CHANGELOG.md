
## [0.1.0] - 2026-02-22

### Added

- Add revision tracking for rolling package builds - ([0e7b88a](https://github.com/pkgforge/sbuilder/commit/0e7b88a9f8cd082a804aab30d6aa099d0c03e583))

### Fixed

- Use recipe pkg name instead of provides entry for pkg_name metadata - ([9a0f8e5](https://github.com/pkgforge/sbuilder/commit/9a0f8e5a0f1bfd5373a6d9515f57b3aa3d879e5f))
- Return all matching packages in name-based cache lookups - ([ff60c11](https://github.com/pkgforge/sbuilder/commit/ff60c11fe42af8d18c31417caaa873cec5cf52f6))
- Use actual pkg name from SBUILD recipe for cache entries - ([c1c8c19](https://github.com/pkgforge/sbuilder/commit/c1c8c19f3794a82ba434ffeecdb813bca2c8070a))

### Other

- Centralize dependencies under [workspace.dependencies] - ([92db2b9](https://github.com/pkgforge/sbuilder/commit/92db2b9d21dc910adbce96cc146b9ba13d959a85))
- Consolidate binaries into unified sbuild CLI with subcommands - ([9818b33](https://github.com/pkgforge/sbuilder/commit/9818b33710e5a53a4b80e7fe86b3e1a9a77ce4ad))
- Remove cache separation, normalize host - ([03bcf4d](https://github.com/pkgforge/sbuilder/commit/03bcf4dbeb9b21dbb87d5a0135043a7d70c65a35))
- Handle remote_pkgver - ([500b1bd](https://github.com/pkgforge/sbuilder/commit/500b1bd3034b9b458b91f43393e245b878a8309a))
- Update, introduce sbuild-meta and sbuild-cache - ([1cc6cd3](https://github.com/pkgforge/sbuilder/commit/1cc6cd399fe16b69eae8dc4895dc52c451453842))
