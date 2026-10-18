# Upstream and third-party materials

Remote Claude is a third-party macOS client derived from [desktop-cc-gui](https://github.com/zhukunpenglinyutong/desktop-cc-gui), based on upstream revision `cc561db` (1.1.4). It is not an Anthropic product.

The upstream README describes the project as MIT-licensed. The 1.1.4 source snapshot does not contain a root LICENSE file. A historical MIT license is available at upstream revision [52259cfba2d5be1216470225c635196129e6abae](https://github.com/zhukunpenglinyutong/desktop-cc-gui/blob/52259cfba2d5be1216470225c635196129e6abae/LICENSE); the subsequent `v1.0.0 init` revision removed that file without explaining a licensing change. This notice records the available provenance; it does not assert that a historical file establishes the licensing of all later upstream code. Confirm the upstream licensing materials before broader redistribution.

Upstream attribution: Thomas Ricouard; zhukunpenglinyutong（朱昆鹏）. Dependency and bundled-resource notices remain with their respective source files, including `src-tauri/resources/agent-catalogs/agency-agents/LICENSE`.

The Claude app icon comes from Anthropic's official App Store artwork. Its cached source and conversion provenance are recorded in `src-tauri/icons/remote-claude/source.json`. The Claude name and artwork are not covered by a new license from this repository. The current package is intended for a private friend trial.
