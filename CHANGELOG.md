# Changelog

All notable changes to logi-mx will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Features

- Add CHANGELOG.md generation to version bump workflow ([62e0a82](62e0a82b2684c302410c73b6e0ca83ce6f8a171a))

### Testing

- Fix test isolation by using unique lock file paths ([5e05bfa](5e05bfa72083cbd41f9232749a37217ce9af5b76))

### CI/CD

- Auto-update PKGBUILD version and sha256sum in version bump workflow ([b17e7f0](b17e7f008538b465a5583858959192d9a06cd9f2))
- Consolidate version bumps into single commit ([1fdbb48](1fdbb485800a447692e435af771d6701617f9bbd))

## [0.3.0] - 2025-11-13

### Bug Fixes

- *(ci)* Correct shell parameter expansion in version-bump workflow ([cd484be](cd484be7e15753175dee3347b046bfa205934412))
- *(ci)* Skip version bump if already at target version ([668344d](668344d38ab9ce4bf5292d45cb1663080ee7ea39))

### CI/CD

- Add automatic version bumping based on conventional commits ([74c706a](74c706ab443b29bbb190215f6fc25d62ed559006))

### Build System

- Update PKGBUILD to version 0.3.0 ([c87ed97](c87ed978239694b973652e1518ed163575564e7f))

## [0.2.0] - 2025-11-13

### Features

- Add professional CI/CD pipeline with GitHub Actions ([92c5396](92c53964cc041addea754db71587efd8e01e4df1))
- Add automated AUR binary package publishing ([ee76961](ee769613291b7250bc745f6df292870ac2f7dd91))
- *(pkgbuild)* Add post-install hook for automatic daemon enablement ([831267c](831267cba5b29cc893592c0fb5a0d218048e30b3))

### Bug Fixes

- Update PKGBUILD sha256sum for v0.1.0 release ([8b7fdb6](8b7fdb6232692912d8224622737d23d381f68edf))
- Add dbus dependency for system tray support ([8bb68fb](8bb68fb18b9827ed7c9c703eeefd88bab91b765a))
- Use ubuntu-22.04 for CI builds ([b9b69cd](b9b69cda8516e74d756e9007588eaedf3e5b6334))
- Exclude UI and daemon from CI builds ([6acb071](6acb0719c5623ccdada0d46b65c720d52f8cf46f))
- Restore latest GTK versions and full CI builds ([a73eb27](a73eb27dc177a0180cd2e8f9e25ba9ae4f256114))
- Set PKG_CONFIG_PATH for GTK/libadwaita detection ([018e0f3](018e0f3198badf2ab7a2c5dab9ebbb1a934ff49f))
- Update tray API and improve UI ([92947a3](92947a3639278dd48af369208879e8afc0bb7234))
- Restore env vars in test and run makepkg as non-root ([0dd674e](0dd674ef74e147416223fafffbff4b263ed2097f))
- *(ci)* Use same SSH key for both AUR packages ([c393e58](c393e58532420821248f6a266b32e02a219fae3d))
- *(ci)* Include logi-mx.install file in AUR publish ([4207f14](4207f144c4a6c6b434caa85cb8223e2dcd2a20c8))

### Documentation

- Add AUR badges to README ([567e0a1](567e0a1b966f73de36e9ae3c40213cac2e76ceff))
- Add comprehensive MX Master 3S hardware documentation ([a0b7ea4](a0b7ea4fdfbf8dfecce77daafab62af582d86ddd))
- Make all README sections collapsible ([5378a78](5378a788c6c877623fe0dfb89a87a77220731d4b))
- Add scroll wheel speed configuration to roadmap ([02adc42](02adc42fbf852cfa90359d19b6114c32ea86d918))
- Add input group setup for out-of-box installation ([82dd0f2](82dd0f26b579b44dcbfc6a7fbed46e6abc0dc3a5))

### Testing

- Add unit tests for scroll_handler module ([4e78f95](4e78f9589705e971e74305ff7a640dc2b468c401))
- Add comprehensive unit tests for traits module ([3b71ff9](3b71ff9c3214ccfc7a4a08425f95c93b561dd551))
- Add tests for MX Master 3S constants and battery statuses ([26af934](26af9344e0bcdbc2f226bf772306181212d57acc))
- Ignore flaky test_config_path_no_env in CI ([b1b66bc](b1b66bc8c96c4ffbc900027ee7d6164bf4aec675))

### Refactoring

- Make config tests deterministic with dependency injection ([1279b27](1279b27fa1d7e22dbb85e6766e2464d4ea93b9f6))

### CI/CD

- Bump the github-actions group with 6 updates ([5183ff6](5183ff65856485764dbb7e51568c7a7fcfd58373))
- Migrate to Arch Linux containers for latest dependencies ([83c8570](83c85706e59bdd3a076d6a1fe466c708f0cc1fe0))
- Replace tarpaulin with llvm-cov for coverage ([7ca87f5](7ca87f5a6139890be217f34b3b236530b9bb4cd1))
- Use rustup for coverage + fix flaky config tests ([295b760](295b7607f991591167540979b281dd9d2eb6b323))
- Add test matrix, caching and Codecov integration ([34263f7](34263f75000157b7c5da53724529517918c80bc3))
- Fix cargo tools cache check to avoid reinstall ([ffc2a68](ffc2a68127a908c7aa2781e2e03ec0d771d9a844))
- Add caching for cargo-audit binary ([17756ed](17756ed9095f43acf3153df5f804890ce599fed0))
- Add professional changelog automation with git-cliff ([12d7e17](12d7e17a8a8b41bc618c93dc7d656ff965dc79c1))
- Remove invalid srcinfo parameter from AUR publish action ([42dbc4c](42dbc4c321efbea051411674cdef39d8b5ccdcb5))
- Add professional secret validation and diagnostics ([eac0cad](eac0cadd3c64356725a1414f3cd2b83a60299589))

### Miscellaneous

- Bump version to 0.1.1 ([199e641](199e6419e55081de2acfa6a8f65047c4ef285d19))
- Update PKGBUILD sha256sum for v0.1.1 ([860157c](860157c9a2928c608e0c358074407da58cf3c2da))
- Bump version to 0.2.0 ([9f73bb1](9f73bb1db846b80774cc39b05fc9c419d0fcc9bc))

### Build System

- *(deps)* Bump codecov/codecov-action from 4 to 5 ([ec0b5c3](ec0b5c3b6704d4c9bcc2a2afa35e5eb2f98f968b))
- *(deps)* Update toml requirement from 0.8 to 0.9 ([312d815](312d8155ac468846c2339367c9ca0e2d4129d5d0))
- *(deps)* Update ksni requirement from 0.2 to 0.3 ([e0f621e](e0f621eabce71cee4f4a09ed877bcae8dbd11b3f))
- Use rustup in PKGBUILD makedepends ([5bcd523](5bcd523cf62f8001724c6e6687647476735ae5c9))
- Update PKGBUILD to version 0.2.0 ([067b20e](067b20e75afeb6680e098dfb8a5a1188a3e7b89b))

## [0.1.0] - 2025-11-11

### Features

- Initial implementation of logi-mx driver ([7bfc806](7bfc806cfd4ef9afc4cc92e97d68665df8546e14))

<!-- generated by git-cliff -->
