# Changelog

All notable changes to logi-mx will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Features

- Add IPC protocol and socket helpers ([1eeb128](1eeb128167b6cfd8c4e056589d60dbdd3c1a242c))
- Wire IPC server and migrate CLI/UI to RPC with direct fallback ([ebac0f4](ebac0f4705a81933fa80580a9eac71399a47d53f))
- Add autostart, daemon health, button divert commands and docs ([f56d265](f56d26514ef6ae4361de514e18999084e286430f))
- Add Easy-Switch host switching command and IPC support ([0ec4813](0ec48133187304d20621fdb2e78dfa1679e30c1f))
- Add unified installer with DE detection and autostart ([9395f67](9395f6766a2c5f40d9854fd2eb8a488abb37af65))
- Improve UI accessibility, battery level bar and tooltips ([614d645](614d6452735c16b61667854899aa9881f4724a7f))
- Add layer-shell overlay button mapper and modernize UI ([79e6d6f](79e6d6f1b214adea0aa795f94f36eeed4b07b166))
- Desktop notifications, unified worker loop and CLI/daemon lint cleanup ([05fa597](05fa5971da3454d106a8005ccd2e46ccbdc64419))
- Render reference SVG device layout in button-mapper overlay ([3ed4328](3ed4328f3e47e160596b43a0598632a707b70524))
- Tinted monochrome tray icon via SNI IconPixmap with per-state colors ([063b6cc](063b6cc9048d4c2c83634a7cfe77bb6bdce299a3))
- Show daemon version at the bottom of the tray menu ([6bfe9aa](6bfe9aa65251b26fa7565f89f126cab673cf0c3c))
- Add Hyprland integration button to the settings window ([4a0e75e](4a0e75e8eaded9d2575ffd99a65827b639c823fd))
- Toggle the overlay on repeated activation and close it with Escape ([6720a9b](6720a9bdadfd918823d3948fc05115c9af38c110))
- Adopt systemd socket activation and runtime watchdog for the daemon ([beab269](beab269d7598ad0cda9f3c20ae846b6db73e723f))
- Add tray restart item that cycles daemon, driver, tray and UI ([0a0af37](0a0af379e05d27a3930bb27b8aa1be4503339b0e))
- Make the overlay background transparent and split leader lines to both sides ([643a515](643a5158207a99f1c05af82f011845b63b8866e4))
- Call out overlay buttons from SVG anchors with side labels ([c8e116b](c8e116be1dc9e32a7360207252ff2353c13e4944))
- Add headless --integrate-hyprland flag and offer it during install ([f6e7a2f](f6e7a2f4362b164215f9f3a7e52e46af2f676f58))
- Stream live input telemetry from an evdev tap over IPC ([fd09ea8](fd09ea84c0cb91a367df54baa051cfc90e2aaebe))
- Unify the UI into a single live HUD window ([b59fe4a](b59fe4a0257e1285bd8f1bb32bf28b5ddeb11022))
- Shade the device diagram with gradients, rim light and wheel texture ([12ba71e](12ba71eb2312e2ea7f8645b11496d3d3184e6a9a))
- Draw the tray icon from the MX Master body silhouette ([4cdd9cd](4cdd9cdf641d655661f1e08dea062a9d1eaca9db))
- Remap reprogrammable controls on the device via HID++ 0x1B04 ([d8e5543](d8e554359418085e1164da0093a3ca2e850f5952))
- Cut button slots into the tray silhouette as a symbolic mask ([7d1df96](7d1df96a6867285cc7436f78769c1451f4b1b340))
- Rebuild the assignment page as a button list with grouped actions ([16704a7](16704a73f3b569bc161d0240b683ff4b3bdb2879))
- Chip callouts, backdrop dim/blur modes and coalesced live wakeups ([c1a1b0f](c1a1b0face68962988cbd22b41df6d343bc84f34))
- Full-screen backdrop with dim/blur checkboxes and tap flash mask ([c4e1915](c4e191513686b1bb76ba488981eac7474ca34b87))
- Two-way highlight between diagram buttons and the Buttons list ([46925a6](46925a648f9988a7d5dae6762d89ff11ac30c855))
- Connect diagram buttons to settings rows with leader lines ([77e80f4](77e80f44debada3543d8d5f63019508cb5629ff8))
- *(ui)* Lay the button tiles out in two columns ([eec8057](eec80572c552613477749f9137e27e5e1a3497d6))
- *(ui)* Give every button tile its own assignment dropdown ([d6dec94](d6dec9401078a6d8844777779cb5caa49e7580ff))
- *(ui)* Stretch button tiles to fill the panel height ([e49564e](e49564e63fb397467c2660efb0c12bc226cc5d7e))
- *(driver)* Decode reprogrammable-controls diverted notifications ([2079a03](2079a03a9e709c80104081b79a33964ed877aaea))
- *(daemon)* Add deterministic swipe recognizer for the gesture button ([2245c7d](2245c7d685ee04d6a3364efe9660d7d197287044))
- *(daemon)* Add lazy uinput key injector for gesture actions ([a140f01](a140f01b97d316a05c483d9798c19f26b38c1a92))
- *(daemon)* Wire gesture recognition into the device worker ([2d6ff93](2d6ff93338616faa19ea5d7fe06fa66253ac53b5))
- *(daemon)* Add gesture IPC with live apply and persistence ([5ec51f2](5ec51f2e8acc04972c830bb6990ae0210b193ab3))
- *(ui)* Add Gestures page with key capture and polish panel scrollbars ([d4739bd](d4739bd2015ac165403659056ea30736e318e3c8))
- *(setup)* Grant uinput access and add setup diagnostics ([be859f9](be859f920ba66f6c2b0b59b60766b891cb65e1d6))
- *(ui)* Redesign the panel header, tabs and tiles ([aaee629](aaee6296cf80f381972005404f2ff73b3df68d20))
- Run shell commands as gesture actions ([68673bc](68673bc5fa6c733f786e32e4f972fa026522507c))
- Bind gestures per application through focus-aware profiles ([a9a54f5](a9a54f568c38d55fbd49a5b01f8a314afed857ac))
- Remap thumb side buttons per application ([40993bc](40993bca3703b19137f1e5852280ffb9a6c16a97))
- Pick existing Hyprland keybinds for gestures and thumb buttons ([7ec589a](7ec589a132cc89a2110fd48a64347b5325406dd4))
- Radial pie menu core in the gesture runtime ([2f1ad58](2f1ad581173221d882c3cc65d0a0f55957e573ea))
- Render the radial pie menu in a dedicated overlay ([260bf58](260bf58d97a47c51ad9926fb29e37c8888e20832))
- Read firmware remaps into the button tiles ([84548eb](84548eba9445e671f9bcc09a2b1d1c4b618c5dfc))
- Charge-gated pie opening with double click and gap-free wedges ([4df0604](4df0604a70e348882ea382b7c6f07794b1eda4ea))
- Continuous aim needle and sliding sector highlight ([df8fb6b](df8fb6bb32db67d24e73c2db7a617c8b26918532))
- Label pie sectors with the bound action summary ([269f06b](269f06bd6d28c8063c3912bcbf234c1408f73a90))
- Pie page with live wheel preview and tuning ([b395097](b395097c791b80404ac87fba079a231ad8b371ea))
- Sector editor on the Pie page ([03f2a9a](03f2a9a136246e55525aef942009c7042015fb8f))
- Paint application icons on pie sectors ([40715a9](40715a989d3ae0f071eeeb447da2585bfafcc2d7))
- Add Diagnostics page with a journal log viewer ([f9fd05f](f9fd05f782b9535b4a9f683afbfe1720848d4dbd))

### Bug Fixes

- Wire packet tests and avoid error-marker collisions ([e70b0c7](e70b0c7c4b56784c3974edbdf34d30d0f972c2cc))
- Harden daemon lock, HID path and UI env sanitization ([ea24013](ea2401319400aa0477cb76227114fb3869eda165))
- IPC error handling, device reuse, status refresh, perf and deps ([6bd143e](6bd143eaf45163732a8302ba84285b803e63a4d1))
- Enable autostart by default in installer ([aad419f](aad419f81ab85d3ecf40a8a22d00dddd811bd5b5))
- Auto-update tray status after device refresh ([9555832](9555832c92a21b6a3be3ae8a72a663fd7fc3ad64))
- Improve AUR packaging with desktop files and binary PKGBUILD ([b5a7db0](b5a7db0668be21bb36529658bbd1b96a10ce9d54))
- Embed battery level bar in row and use valid DPI icon ([d15c51b](d15c51b95130cfc6d92f4b3b0af0ba1f88eaad6c))
- Supervise tray registration and retry when watcher unavailable ([ecb964a](ecb964ae822b452143413620a2a3bea3cb2bb532))
- Check desktop session before tray/UI and use event-driven tray ([ac98874](ac98874b43e7c6cc36333fd60bfea796d1d08204))
- Position overlay surface via layer-shell margins and clear clippy lints ([6cf4ea0](6cf4ea0952c21cc1e515dca9eda1ec66b99aa5c3))
- Never block the tray service task and reap the launched UI ([adf8f99](adf8f999511a205787c4352be5330dc19b5f9f71))
- Harden daemon against panics with bounded runtime and safe thread spawns ([5325a7d](5325a7d7c9aba258518f8b6ddf2782aaf0b1d0cf))
- Publish tinted pixmap as primary tray icon, theme name only as fallback ([5d63e45](5d63e45fde27eb1007ac9ffc0ba04516ce82eca8))
- Tray menu shows bare version string without separator or icon ([bf9d81a](bf9d81aec0f557dd3c7aec201d9f2813a33c869d))
- Center the version line in the tray menu and clear clippy lint ([1b24495](1b24495ce8a001acda00d3c5dcb1cbf86ddbae50))
- Drop space padding, render version as a plain left-aligned item ([7e1a101](7e1a101337d1524099b2348ad1d46f33d7fd9627))
- Give the overlay its own application id so it launches independently ([71e7184](71e71847360e9aa6a0adb692a2e3bcda0f21e163))
- Drop the pointer speedometer gauge from the HUD ([7e8d7f3](7e8d7f35dd45f1d64de16877238afa50935734cf))
- Keep toggle closed when the HUD window exists ([2959e28](2959e285fd983a7f082f9e6b044c2d211b9b6c1a))
- *(ui)* Highlight buttons by exact SVG shape instead of bounding rectangle ([6afc2f2](6afc2f299869861bb44aebc880322c50f7d66770))
- *(ui)* Present the HUD instantly and load the device snapshot off-thread ([8bc3117](8bc3117c37eb41444b1413d9a0b3e34c9fcf1322))
- *(ui)* Request compositor blur through the ext-background-effect protocol ([6c8438e](6c8438e8da6c84bb94242cf0cd72deece645a0ae))
- *(ui)* Make the remap page honest and legible ([02f20ef](02f20ef5fd7f1bb61644a077ebccd3a0b418204a))
- *(ui)* Show each button tile as button and assignment side by side ([b3a96d4](b3a96d4fffa23c4e6ccca691024a2ef1b7bc1ef8))
- *(ui)* Name the remap section plainly ([615d1ca](615d1cabbe0266161f2681abf0070c7bc66eec7c))
- *(daemon)* Keep the live telemetry stream across event batches ([f1b4fa6](f1b4fa620c49cb2979f36e610ac72c0a879fc77f))
- *(daemon)* Own one HID++ handle per device and discover at startup ([8780371](878037171c7ed1d14450e9bd9df008254548974a))
- Correct thumb-button highlights and light up gesture and wheel-mode ([bb4faea](bb4faea0867a97201c48343ccbcb6ac593a3c6b0))
- Discover the Hyprland instance when the environment lacks it ([1ef9b91](1ef9b919e8c86baba8d98bbf2e1748e7398087c2))
- Query the Hyprland request socket by its real name ([6ed2ac2](6ed2ac27ffa1d0da2e23505cf332ed5a8bd171e4))
- Parse the plain-text activewindow reply for the initial class ([e1f29f2](e1f29f21860b2e150c259847933504d02a57826d))
- Persist device settings so they survive a daemon restart ([f5ae7f9](f5ae7f91150bf1fdf2a5817adf64713c58bdeac7))
- Open keybind picker as layer-shell overlay above the HUD ([32c0286](32c0286d39a7ded439c46427bb3e8282d1080b78))
- Address CodeRabbit findings in daemon and driver ([fffa64d](fffa64d3add48057b9ba7a629895cfe285c00697))
- Address CodeRabbit UI findings and page spacing ([87b6e76](87b6e76d9aaabc27c66182ac0ad2954b63fc469a))
- Address CodeRabbit packaging findings ([6ce6c3d](6ce6c3d09f3f43a5763f696e723c23b189782123))
- Address CodeRabbit findings in the CLI ([031f9be](031f9be722cb5933743ab86926158187c74e7a10))
- Anchor HUD toasts to the top and silence the false restore toast ([e5602b9](e5602b970ab35114227b4b51ee73308df04692d5))
- Keep HUD toasts on a single line ([3111d63](3111d6332cc9cb7de85dce8f6935d9709d758ea3))
- Open the pie ring on press and select it by wheel swipe ([444761b](444761bce9d07c536ada54c585d06409d90d8bc8))
- Repaint the surface once the exit animation ends ([8b823ea](8b823ea87388670ee50b4f747b534c510cd2d9f2))
- Keep clippy clean when the tray feature is off ([94a517c](94a517c28c96d7d6c781a64e7993e490b42e90f2))
- Wake the device worker the moment an IPC request arrives ([acfbe9b](acfbe9bc20b4cffd9808ab5573b6a1bb05e8095b))
- Open the HUD without waiting on device I/O ([c67d2ca](c67d2ca74dbb9c07d5255dda846cd1cc57491639))
- Harden pie commit, release timing and late subscribers ([023b67f](023b67f90d57cfba5512efdbff56ef6fc7b031d1))
- Thumb-button recovery, tray honesty and pie stream integrity ([f407caf](f407caf9d65a77629097779235ebef672d3d37dc))
- Overlay Wayland gate, monitor hotplug and wedge-fitted labels ([3b6d610](3b6d610eaa8d01ca908cb4083dfd73b366229883))
- Tile pie wedges by bound sectors and harden the pie editor ([3c01d9b](3c01d9bab09bd1fdf9e8363bc3f2a5bfc9a084e1))
- Keep editor state honest when daemon reads and writes fail ([2c5f33f](2c5f33f5a61b145fcbbf89f733bb84312df62fb1))
- Recover the header chip, snapshot-backed controls and pie page ([d93ef65](d93ef651f17940f208e65c933c500af1ff4e663b))
- Bound the SmartShift threshold to the documented device range ([b8aaa51](b8aaa517a017bba8acaf892fbd694328df61a4d4))
- Honest autostart lifecycle, installer ownership and CLI reporting ([9cc3952](9cc3952f8970554bc52798ec1abc4e765d5ca748))
- Reject pie tuning that would make the menu unusable ([bc138f1](bc138f134f8258acdbf421b77701fcf7ad299e35))
- Stop the tab icons spamming GTK baseline warnings ([daad941](daad9414035c59814d788bb7630ac881d9e049cd))

### Documentation

- Update README, INSTALL and ARCHITECTURE for new features ([df5d926](df5d926818424cef061a7df2e550e81b4503f1d3))
- Update Hyprland config section for Lua (0.55+) ([676c1cb](676c1cb4141ab645f3b0c3852df87f60ead5cef9))
- Describe the in-app Hyprland integration button in README ([c4cd02a](c4cd02af58daa021036cc3a38c26b784d296cdfb))

### Refactoring

- Remove redundant get_ prefix per RM-STRUCT-002 ([62d3d21](62d3d21f39050602cfa11bf81a1509c847c824e1))
- Buffer stdout in CLI to avoid per-line locking ([d001807](d001807682befab9bd1c9035ed71927ed86dd517))
- Extract HidppHeader to deduplicate packet fields ([f3e72d8](f3e72d84f683adf0466b2fe912e15115332f47f5))
- Deduplicate battery parsers and tray status refresh ([1d8a219](1d8a219e40b1173a6c5c0bd9a532f9f59924a38f))
- Delegate constructors to primary new ([77c95f1](77c95f13a37d6948ba4b4049ff866ae172d592f6))
- Extract UI constants and document groups ([1c0c0b3](1c0c0b383dea9f3a7676465f4f5346b1b494c181))
- Modernize UI with AdwToolbarView and WindowTitle ([9ec7ae3](9ec7ae3d3b39faf0c446812f16fc9a987279fb6a))
- Remove body comments, expect and blocking tray callback ([1ca9a40](1ca9a406faeb3c0b5c4d9f296665be2857cfb07d))
- Split UI into snapshot, settings and overlay modules ([d741cb8](d741cb80891aa096508c298e676ff01cb67d4d81))
- Drop the live telemetry line from the panel ([5d80547](5d80547aad8b65eb5671d0f86241dd70a70be374))
- Split oversized modules into focused submodules ([3f3e662](3f3e6624ddcc3fa4170034c3f89d302101977fc3))
- Uniform tray types across feature combinations ([6dc1470](6dc14705f97dbaf216eb8755bf1ee3342abc6435))

### Styling

- Apply cargo +nightly fmt ([c81de2f](c81de2f36444c22aa17dc6059371f1ca838c26be))

### Miscellaneous

- Bump masterror 0.30 -> 0.31 ([47f56df](47f56df89c2db0fc4e6074b3c85170e8c0102242))
- Up deps ([e2dccef](e2dcceff86cfbd2b3c0289566bed1e59c412d671))
- Add justfile for professional local workflows ([4e3cf07](4e3cf07174d7a9bb7eff7a3390132f2d0ccaaa20))
- Bump SPDX copyright year to 2025-2026 in touched files ([25adfe7](25adfe770d4fb15ee93734e273a486bc9dc73978))
- Bump workspace version to 0.6.0 ([d339b17](d339b1769fc8e981df07f38aea218d840ad000b6))

### Debug

- Trace the radial pie menu chain end to end ([17e44e1](17e44e11501ab64d787b42b5cee505c1f9a0fd0d))

### Polish

- Compact springy pie with smooth exit ([4912b96](4912b9650aea010b5540bf7fbd7749d0e4068fde))

## [0.5.0] - 2026-09-01

### Features

- Auto-discover paired device on Bolt receiver slots ([07bf1a6](07bf1a642e0f18bd0e1a70e0b912f365d5fbd9e9))
- Buttons and hosts CLI commands, business variant name matching ([cb50585](cb50585fcff5ce67053ecdaf43e5adaa26b83c88))

### Bug Fixes

- Enable masterror std feature broken by no_std introspection path ([f460730](f460730b34611a1c39c7d3186a7c1d975a558796))
- Silence tray-gated warnings in daemon without tray feature ([1826d48](1826d4878aedb1f48baa4f5c8579a973987508c9))
- Align HID++ driver with verified protocol semantics ([dd3db5b](dd3db5b3d6622f5dd77eb88b46495eadeff0aa8f))
- Drop --locked from MSRV check since Cargo.lock is gitignored ([428898c](428898c55615f7cc697415f480354ea6eb0db02b))

### Performance

- Collect one device snapshot per UI launch instead of five opens ([5f98b51](5f98b51efb8f17c6aedbfab5d2ebebffe3d44591))

### Documentation

- Align README with implemented feature set ([988093a](988093a783607e70d2c0910593b419ee06343b95))

### Testing

- Add proptest property tests for HID++ packet codec ([914a725](914a725f6cad1929546789ad5a775fc597794702))

### Refactoring

- Enforce pedantic and nursery clippy lints across workspace ([14d0af9](14d0af999134f76343685a7d7eef55296c3fe350))
- Move blocking device I/O to dedicated worker thread ([608c2f9](608c2f98348d45e8e09748f35af4eccf21cc1a07))

### CI/CD

- Bump the github-actions group with 3 updates ([#49](https://github.com/RAprogramm/logi-mx/issues/49)) ([430f252](430f2523a236ce30fca4bed45b85c1bb856db748))
- Add cargo-deny, MSRV gate, clippy pedantic and release checksums ([c278270](c278270eec4cff19ab14e2aa94c29310444ed3f2))
- Run doctests and add release build gate ([e5066e9](e5066e9f8eb7c3e5333f3386fdfa728bac446db8))

### Miscellaneous

- Up deps ([a5f5c01](a5f5c0193fb649f11f3a455c35b1df2a87827dd7))
- Remove unused dependencies detected by cargo-udeps ([817f43b](817f43b7bd7067a07f61028db2e5264c85d9a1dd))
- Bump version to 0.5.0 ([13a6343](13a63439caf2a04335ff62128c23451bfd783fe4))

### Bench

- Add criterion benchmarks for packet codec and CI bench job ([4c6dfb8](4c6dfb8c04c8e5175f9d11d105f1311d32d753e6))

### Deps

- *(deps)* Update masterror requirement from 0.26.0 to 0.27.0 ([#50](https://github.com/RAprogramm/logi-mx/issues/50)) ([cd1595e](cd1595e12d9448981ff89be8fa95c8fe0cd33e0c))

## [0.4.6] - 2025-12-12

### Bug Fixes

- Fix wheel test 1 ([67e6530](67e6530a4c9b3ca7b75ab437c1255888250aa5f9))
- Remove scroll handler that blocked mouse wheel ([2470da2](2470da2e24f11cab37ba42094fd547ddc34355de))
- Improve cargo tool installation in CI ([c39d6cf](c39d6cf0d09c7cca4a6483dddfff799a844b23f0))

### Refactoring

- Remove retry hacks, use event-driven architecture ([a6b9d65](a6b9d65742920956cfac54675d14a4dfa6435632))

### CI/CD

- Bump the github-actions group with 4 updates ([#48](https://github.com/RAprogramm/logi-mx/issues/48)) ([d8fdb7c](d8fdb7c6b575ac03d73c5c7cecc64e56c394e41f))

### Miscellaneous

- Bump version to 0.4.6 ([2da8f9c](2da8f9cfd45c33265864500e0fa21fb5567d3c81))

## [0.4.5] - 2025-12-04

### Bug Fixes

- Auto-restart scroll handler on device disconnection ([9861601](98616014c62655fc115370f4d3c67b94a4b526e8))

### Miscellaneous

- Bump version to 0.4.5 ([1508f32](1508f32b1b41e48605df5eab5bd0f64c1c96fc58))

## [0.4.4] - 2025-12-02

### Bug Fixes

- Correct archive name in PKGBUILD.bin (x86_64-linux-gnu) ([9d8f86b](9d8f86bc1d4e0f828396f6393a2030a03d567392))

### Miscellaneous

- Bump version to 0.4.4 ([51cf2ea](51cf2eaf0415fb4523f44a93367cb221d939e56b))

## [0.4.3] - 2025-12-02

### Bug Fixes

- Use custom script for logi-mx AUR push (handle detached HEAD) ([83f1f12](83f1f12c48cc2b00f9cf837c3c2a60dae526af9e))

### Miscellaneous

- Bump version to 0.4.3 ([cd565c3](cd565c34a0acd974746ea557d1b5e99352e7ee01))

## [0.4.2] - 2025-12-02

### Bug Fixes

- Add force_push for logi-mx AUR package (missing HEAD ref) ([18071ec](18071ec212727b60450879afb4b5c36410f66736))

### Miscellaneous

- Trigger CI for AUR sync ([30657c6](30657c61f538e2778f70e42e6c2e8f9921878efb))
- Bump version to 0.4.2 ([7ef7614](7ef76147f9aa5374c3ccbc05a4d65f47d0d4c571))

## [0.4.1] - 2025-12-02

### Bug Fixes

- Prevent race condition between publish and sync AUR jobs ([#47](https://github.com/RAprogramm/logi-mx/issues/47)) ([2b2fb1b](2b2fb1bf51e98393cd79ee5781078882e656b77b))

### Miscellaneous

- Bump version to 0.4.1 ([ab32e7b](ab32e7b72216760285c704e080df7a6385b701dd))

## [0.4.0] - 2025-12-02

### Features

- Add AUR sync check and auto-publish if behind ([#46](https://github.com/RAprogramm/logi-mx/issues/46)) ([628a016](628a016b5179ad7fc93f97a491a9680b7860bd3f))

### CI/CD

- Bump actions/download-artifact in the github-actions group ([#38](https://github.com/RAprogramm/logi-mx/issues/38)) ([f938228](f938228659014c28367895308315fb22c0cb38a3))
- Bump actions/checkout from 5 to 6 in the github-actions group ([#39](https://github.com/RAprogramm/logi-mx/issues/39)) ([0ddbbcb](0ddbbcb7b24bd8e3d20722d3b2f0eecbd3853134))
- Fix release jobs to run after version-bump creates tag ([994b957](994b957012de5112b12ce2e96bafd491fa69e562))
- Fix checkout ref and add explicit tag_name to releases ([938d8fd](938d8fdee5c9e81b640102479b722c6e169f00b4))

### Miscellaneous

- Bump version to 0.4.0 ([ffa3869](ffa386990465c320d2925d719754f51a78fe4d55))

### Deps

- *(deps)* Update masterror requirement from 0.25.1 to 0.26.0 ([#40](https://github.com/RAprogramm/logi-mx/issues/40)) ([86bbf03](86bbf0337b10aa23b4e95e8c3dd52322f137945c))

## [0.3.1] - 2025-11-14

### Bug Fixes

- Prevent zombie processes and ensure proper UI cleanup ([#37](https://github.com/RAprogramm/logi-mx/issues/37)) ([683fd8e](683fd8e4d6f0125b76c4fed70169e9b90ebd17a7))

### Documentation

- Add AUR badges for binary package ([ecf75f7](ecf75f7e0f27c6e696561cb7492ba05d16320642))

### Refactoring

- Consolidate version-bump into CI workflow ([e4d9069](e4d9069bec8bc467717675fcb1a470e6c5304489))

### CI/CD

- Auto-update PKGBUILD version and sha256sum in version bump workflow ([b17e7f0](b17e7f008538b465a5583858959192d9a06cd9f2))
- Consolidate version bumps into single commit ([1fdbb48](1fdbb485800a447692e435af771d6701617f9bbd))

### Miscellaneous

- Bump version to 0.3.1 ([29bebda](29bebdab137df23848957d94cfb10d194e66ae65))

## [0.3.0] - 2025-11-13

### Features

- Add exit confirmation dialog to system tray ([a876df2](a876df27c682831accbcea48b93b4890e1c83534))
- Add desktop launcher for daemon ([ccd014c](ccd014cbc1e9d7e82d7ee30eaaa6b1e1ea0a2361))
- Implement proper daemon shutdown via systemctl ([b64a8a4](b64a8a45befa09013a3e2b1b16d943fc9f6683c2))

### Bug Fixes

- *(ci)* Correct shell parameter expansion in version-bump workflow ([cd484be](cd484be7e15753175dee3347b046bfa205934412))
- *(ci)* Skip version bump if already at target version ([668344d](668344d38ab9ce4bf5292d45cb1663080ee7ea39))

### Documentation

- Add native vs daemon mode comparison section ([bd074f0](bd074f012bbb2c66943e51325f540b066cd107c2))

### Testing

- Add comprehensive unit tests for tray status indicators ([d42abb0](d42abb05cf707b546796ae5583648098aa4fed8e))

### CI/CD

- Add automatic version bumping based on conventional commits ([74c706a](74c706ab443b29bbb190215f6fc25d62ed559006))

### Miscellaneous

- Bump version to 0.3.0 ([9162f5e](9162f5e71463603cacb97f6cb611324220d0a446))

### Build System

- Update PKGBUILD to version 0.3.0 ([c87ed97](c87ed978239694b973652e1518ed163575564e7f))

## [0.2.0] - 2025-11-13

### Features

- Add professional CI/CD pipeline with GitHub Actions ([92c5396](92c53964cc041addea754db71587efd8e01e4df1))
- Add automated AUR binary package publishing ([ee76961](ee769613291b7250bc745f6df292870ac2f7dd91))
- Add scroll wheel speed configuration ([69a1532](69a153229202d30bc83631996fabb8fd653286b3))
- Implement daemon scroll speed multiplier with fractional support ([4087a60](4087a60c2ffd8a594696c32d243b736feb5848af))
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
