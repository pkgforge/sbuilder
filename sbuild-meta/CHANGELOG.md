
## [0.1.0] - 2026-02-22

### Added

- Add x_exec.container, build_deps, and checksum_bsum metadata - ([2f0964f](https://github.com/pkgforge/sbuilder/commit/2f0964f78e295687f2006444655a24c73715a2e2))
- Add packages field for multi-package recipes and fix push logic - ([76bdd66](https://github.com/pkgforge/sbuilder/commit/76bdd66283e4736d041060bca93e80a6b53aeb2d))

### Fixed

- Use recipe pkg name instead of provides entry for pkg_name metadata - ([9a0f8e5](https://github.com/pkgforge/sbuilder/commit/9a0f8e5a0f1bfd5373a6d9515f57b3aa3d879e5f))
- Fix generate metadata - ([7237a8c](https://github.com/pkgforge/sbuilder/commit/7237a8c200eea97e3c6fca046a328c8f931d7050))
- Fix metadata, minisign, rank by downloads - ([441ca5a](https://github.com/pkgforge/sbuilder/commit/441ca5a3c7ecd2fed8d80599480616ee15991b06))
- Fix download_url in metadata - ([91901a1](https://github.com/pkgforge/sbuilder/commit/91901a1a053281943b876321b19ab1bd17cc1761))
- Fix uploaded file - ([7f881bf](https://github.com/pkgforge/sbuilder/commit/7f881bf947f4bfb7545a715e2b6c6351a9fa2484))
- Fix ghcr path and version - ([11bc4c0](https://github.com/pkgforge/sbuilder/commit/11bc4c0f95d90f990c35efefc43b952268ede333))

### Other

- Replace serde_yml/serde_yaml with saphyr and simplify schema - ([2f1f48e](https://github.com/pkgforge/sbuilder/commit/2f1f48e053ade96a5614e86170202ffb80c3ea5c))
- Centralize dependencies under [workspace.dependencies] - ([92db2b9](https://github.com/pkgforge/sbuilder/commit/92db2b9d21dc910adbce96cc146b9ba13d959a85))
- Remove duplicate code and dead functions after CLI consolidation - ([1e400ac](https://github.com/pkgforge/sbuilder/commit/1e400ac379205cd2925f537fbd45aa4c1140aa68))
- Consolidate binaries into unified sbuild CLI with subcommands - ([9818b33](https://github.com/pkgforge/sbuilder/commit/9818b33710e5a53a4b80e7fe86b3e1a9a77ce4ad))
- Add @name syntax for binary-only provides entries - ([80fe9f6](https://github.com/pkgforge/sbuilder/commit/80fe9f616c710ec8d4b19d89f01366fce314830c))
- Remove build restrictions on binary types - ([346822b](https://github.com/pkgforge/sbuilder/commit/346822b4e93bdc4e8a9f9c27f0a25dcfe988cce6))
- Remove cache separation, normalize host - ([03bcf4d](https://github.com/pkgforge/sbuilder/commit/03bcf4dbeb9b21dbb87d5a0135043a7d70c65a35))
- Update version check - ([723868b](https://github.com/pkgforge/sbuilder/commit/723868b7358730576de0750c22d3c90de2a3043a))
- Handle remote_pkgver - ([500b1bd](https://github.com/pkgforge/sbuilder/commit/500b1bd3034b9b458b91f43393e245b878a8309a))
- Use annotations for metadata values - ([c361752](https://github.com/pkgforge/sbuilder/commit/c36175267d54213cfa5f3baf2b2efc1e8707b432))
- Determine cache type based on pkg_type - ([1b420de](https://github.com/pkgforge/sbuilder/commit/1b420dea52e213062698cfa86b4dd977bb360398))
- Remove cache type requirement in ghcr path - ([875b25f](https://github.com/pkgforge/sbuilder/commit/875b25fb0e00cc85c348827cd3ec61846a8a1f12))
- Set default log level to info - ([0db7b25](https://github.com/pkgforge/sbuilder/commit/0db7b256c1c2ab055082b47f96917637ec86d028))
- Make ghcr_pkg in sbuild portable - ([cf97c6d](https://github.com/pkgforge/sbuilder/commit/cf97c6dc585e6479f830d525192d4ca3b5ee476c))
- Use version placeholders - ([48f7e58](https://github.com/pkgforge/sbuilder/commit/48f7e58c944da5b9cb88ea3198332dde96c349fa))
- Handle snapshots - ([a0f1314](https://github.com/pkgforge/sbuilder/commit/a0f13143f3e52be0c218543dc6153007f8d4f8c6))
- Remove rank, download count - ([93720c8](https://github.com/pkgforge/sbuilder/commit/93720c8eb4fe34731b34f0bc0d387ae1746e9cb4))
- Handle provide in own directory - ([afc3afc](https://github.com/pkgforge/sbuilder/commit/afc3afc8624c0cb49500811fa5e2030a147a00be))
- Update manifest - ([cd43725](https://github.com/pkgforge/sbuilder/commit/cd4372526c38cb4017a77a117e8a84cc78be71b4))
- Update - ([6c7c7cb](https://github.com/pkgforge/sbuilder/commit/6c7c7cb287850fd4abfc7ae467f7ce9498d7d47a))
- Update, introduce sbuild-meta and sbuild-cache - ([1cc6cd3](https://github.com/pkgforge/sbuilder/commit/1cc6cd399fe16b69eae8dc4895dc52c451453842))
