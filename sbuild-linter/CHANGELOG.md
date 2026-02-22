
## [0.5.0] - 2026-02-22

### Added

- Add x_exec.container, build_deps, and checksum_bsum metadata - ([2f0964f](https://github.com/pkgforge/sbuilder/commit/2f0964f78e295687f2006444655a24c73715a2e2))
- Add packages field for multi-package recipes and fix push logic - ([76bdd66](https://github.com/pkgforge/sbuilder/commit/76bdd66283e4736d041060bca93e80a6b53aeb2d))

### Fixed

- Write remote_pkgver to pkgver file when defined in config - ([d47bfbe](https://github.com/pkgforge/sbuilder/commit/d47bfbeb86c1d8d01924206d350e3805b18350cd))
- Fix snapshot and ghcr_pkg handling - ([3f9dc5e](https://github.com/pkgforge/sbuilder/commit/3f9dc5e71bd2ad60ac55707e2b641647815f8672))
- Fix temp file and update sbuild file logging - ([1c85eca](https://github.com/pkgforge/sbuilder/commit/1c85eca2d7aedf66f9e7713004b331dd018b518b))
- Fix sbuild and update workflow - ([6456561](https://github.com/pkgforge/sbuilder/commit/645656121abf239c381747bd065de82ac6ad23f8))
- Fix pkgver - ([b3e27b2](https://github.com/pkgforge/sbuilder/commit/b3e27b26416827139c8605499de803678161edaf))
- Fix validated output, add multi-file support - ([f908efd](https://github.com/pkgforge/sbuilder/commit/f908efd44fa3b91bb7daa6958b7fd30546ad3272))
- Fix multi-line output - ([d1a60b9](https://github.com/pkgforge/sbuilder/commit/d1a60b9769e0057d5a53498c42533e1ee48b4e59))
- Fix validated output identation and string quotes - ([9f6fc17](https://github.com/pkgforge/sbuilder/commit/9f6fc172f62c6eae0d1260135952ad8d2266d15f))

### Other

- Make run optional - ([b7ff918](https://github.com/pkgforge/sbuilder/commit/b7ff91876078dfcade44030924c92970cde2537f))
- Replace serde_yml/serde_yaml with saphyr and simplify schema - ([2f1f48e](https://github.com/pkgforge/sbuilder/commit/2f1f48e053ade96a5614e86170202ffb80c3ea5c))
- Centralize dependencies under [workspace.dependencies] - ([92db2b9](https://github.com/pkgforge/sbuilder/commit/92db2b9d21dc910adbce96cc146b9ba13d959a85))
- Consolidate binaries into unified sbuild CLI with subcommands - ([9818b33](https://github.com/pkgforge/sbuilder/commit/9818b33710e5a53a4b80e7fe86b3e1a9a77ce4ad))
- Expose REMOTE_PKGVER - ([c3e305b](https://github.com/pkgforge/sbuilder/commit/c3e305b64b5d2bb9972c0797c8573eaf7d201cf4))
- Handle remote_pkgver - ([500b1bd](https://github.com/pkgforge/sbuilder/commit/500b1bd3034b9b458b91f43393e245b878a8309a))
- Update version - ([ba368ef](https://github.com/pkgforge/sbuilder/commit/ba368ef98da9602089727198f900d70cf099fe07))
- Handle ghcr_pkg - ([be8a7c8](https://github.com/pkgforge/sbuilder/commit/be8a7c8ac2feb57837b13aea86e95e1ed101917a))
- Update - ([6c7c7cb](https://github.com/pkgforge/sbuilder/commit/6c7c7cb287850fd4abfc7ae467f7ce9498d7d47a))
- Update dependencies and release - ([96590b8](https://github.com/pkgforge/sbuilder/commit/96590b81fda153fd191ec262c3c69efddb620b2f))
- Quote non-alphanumeric keys - ([108657b](https://github.com/pkgforge/sbuilder/commit/108657b5aab74900658e5c9ccb29d1a4b380b7ce))
- Parse boolean as string in description key - ([4695270](https://github.com/pkgforge/sbuilder/commit/469527077ff155ee3fca1140cff14810b577430a))
- Use url library for url validation - ([87e96fb](https://github.com/pkgforge/sbuilder/commit/87e96fb2c4ca3f3e019d85f7d493500bd49529f8))
- Handle provides and automate dynamic appimage conversion - ([6cbc70a](https://github.com/pkgforge/sbuilder/commit/6cbc70af94b2653a3c4003627228068e8a731abc))
- Support custom outdir, timeout and debug mode - ([0fd4e14](https://github.com/pkgforge/sbuilder/commit/0fd4e14342b905b7eeb817e380ad7c86e38b36d6))
- Support entrypoint, licenses, and host checks - ([ffe3ea9](https://github.com/pkgforge/sbuilder/commit/ffe3ea9f8a877854639f3fedae2b6c03bd3cdaad))
- Update logger and fix _disabled_reason validation - ([0e2dca5](https://github.com/pkgforge/sbuilder/commit/0e2dca5abe2347c4a44c36ef89557855f7021032))
- Disable app_id generation, add custom timeout - ([2ddf03b](https://github.com/pkgforge/sbuilder/commit/2ddf03b7636d1cd92aaaadd50c0d94c69c015146))
- Fix x_exec validation - ([9f14133](https://github.com/pkgforge/sbuilder/commit/9f141334c7f5ad79068d39c632acdfa84e117b25))
- Extend license and x_exec - ([d466b61](https://github.com/pkgforge/sbuilder/commit/d466b613d7e3a911206141b021f2f5c90cca4b42))
- Add support for _disabled_reason, extended description/desktop/icon - ([ee28ff6](https://github.com/pkgforge/sbuilder/commit/ee28ff65eb4dd94f7d35eb4050308ed1662e3631))
- Initialize sbuilder - ([e688ee1](https://github.com/pkgforge/sbuilder/commit/e688ee17ae8cae9b4ffada40355155b524abcab6))
- Don't overwrite success/fail file, return buildconfig on lint - ([59c6981](https://github.com/pkgforge/sbuilder/commit/59c6981b163556560340662fe676d1db9b63aff2))
- Introduce pkgver fetch timeout, flag to write success/fail to file - ([e82b024](https://github.com/pkgforge/sbuilder/commit/e82b024eacb88d330d204c9b4b9f15b5b14585b7))
- Add support for replacing original file on success - ([2abeae5](https://github.com/pkgforge/sbuilder/commit/2abeae58077b821037c8db896a109bbe55fb5956))
- Sync logging, fix validation to include all error for a field - ([f9278aa](https://github.com/pkgforge/sbuilder/commit/f9278aaf07925a9b82f7673cb217abe9bfb02eb1))
- Refactor codebase - ([e923c61](https://github.com/pkgforge/sbuilder/commit/e923c613dd789bad7dc9ace1f2f1922353b0a8aa))
- Sbuild-linter v0.2.0 - ([17b9927](https://github.com/pkgforge/sbuilder/commit/17b9927e5a59a7f1325489aac0c173ea2e53620f))
- Add parallel support - ([b17af7a](https://github.com/pkgforge/sbuilder/commit/b17af7ac6baebdee973e0893aa2578ecffb9ecc6))
- Write scripts to tmp file - ([b02eb60](https://github.com/pkgforge/sbuilder/commit/b02eb60aa1266f34089d137a0593a3d02dd1b930))
- Remove set -e in script - ([6ce5787](https://github.com/pkgforge/sbuilder/commit/6ce5787453ef72b555a4d553dbf933d89ddf6acb))
- Better error message for invalid files - ([b7502ef](https://github.com/pkgforge/sbuilder/commit/b7502ef9c7973a6872fae757266693f09267fc5f))
- Add release workflow - ([1516d35](https://github.com/pkgforge/sbuilder/commit/1516d35b5722743a83b5a08c3a3c5a517cde50b7))
- Refactor pkgver processing - ([fa62453](https://github.com/pkgforge/sbuilder/commit/fa624535bf7ee5d6a3bd59e904bd1adcb4ce27be))
- Add x_exec checks, fix pkg_id - ([001b9a7](https://github.com/pkgforge/sbuilder/commit/001b9a7620baece8fab69de4ad26b4b9a4999305))
- Reduce unused attributes - ([c5f9233](https://github.com/pkgforge/sbuilder/commit/c5f923398d02ac67235b92ab329c5514c10b1f0a))
- Intialize sbuild linter - ([1e2af7b](https://github.com/pkgforge/sbuilder/commit/1e2af7b6520c443b19c3d435b0c092a322bfc58f))
