# Contributing to Remote Claude

Remote Claude focuses on a macOS client for Claude Code running on a Linux VPS over SSH.
Bug reports, documentation corrections, and focused pull requests are welcome.
中文反馈也欢迎；请提供可复现步骤，并在分享日志前移除隐私信息。

## Report a bug

Check existing issues first. Include:

- The app version or source revision, macOS version, and Mac architecture.
- The remote Linux and Claude Code versions, when relevant.
- The smallest steps that reproduce the problem, with expected and actual behavior.
- The exact error text and stage: setup, launch, streaming, history, files, Git, or terminal.
- Whether it happens with synthetic data, a native build, or an authorized live SSH session.

Use a small example project where possible. Redact hostnames, usernames, private paths,
and screenshots before sharing. Do not attach private keys, tokens, full conversations,
or an entire app-data directory.

## Set up a checkout

Read [AGENTS.md](AGENTS.md) and the [Remote development guide](README.remote-vps.md).
Use the pnpm version declared in `package.json` (currently 10.29.3), Node.js, Rust stable,
and macOS Xcode Command Line Tools for native builds.

```sh
pnpm install --frozen-lockfile
```

Preserve unrelated changes in your checkout. Use the existing
[Remote build script](scripts/build-remote-macos.sh) when packaging the macOS app;
it pairs the `remote-vps` feature with the Remote identity configuration.
Keep `local.ccgui.remote-vps`, `~/.ccgui-remote-vps`, and disabled upstream updates intact.

## Run relevant checks

These are local build and unit-test examples; they require no VPS or Claude account.
Choose checks that cover your change, and report what you actually ran.

```sh
pnpm build
pnpm exec vitest run src/features/settings/SshWorkspacesCard.test.tsx
cargo test --manifest-path src-tauri/Cargo.toml --features remote-vps --lib engine::ssh_onboarding::tests
git diff --check
```

For xterm or terminal bundling changes, run `pnpm build` first, then
`node --test tests/xterm-production.test.mjs`. For frontend interaction checks, see
[browser fixtures](tests/browser/README.md); their simulated results do not prove live SSH behavior.
Documentation-only changes need content and link checks rather than an app rebuild.

Live SSH and model tests are opt-in. Obtain explicit authorization from the machine and
account owner, agree on the host and working directory, and disclose model calls or file
changes before running them. Never select a target silently from `~/.ssh/config`, or run
the full `ssh_live` suite as a routine local check. Follow the Remote development guide.

## Submit a focused pull request

- Keep each PR about one problem; explain broader proposals before implementing a large change.
- Describe the trigger, resulting behavior, validation, and any remaining limitations.
- Add a regression that exposes the bug. For async or process changes, cover relevant
  cancellation, timeout, concurrency, and recovery paths.
- Preserve both local and Remote behavior when changing shared code. Check IPC producers,
  consumers, errors, and cleanup together; do not hide failures with empty success results.
- Keep [English](README.md) and [Chinese](README.zh-CN.md) README content aligned.
  Update `src/i18n/en.ts` and `src/i18n/zh.ts` together for user-visible text.
- Update the [UI specification](docs/ui-ux-spec.zh-CN.md) for interaction changes and
  register new refresh actions there. SDK changes also require `pnpm plugin-skill:docs`.

## Keep the repository shareable

Do not commit credentials, `.env` secrets, SSH identities, private project data, session
caches, app-data directories, diagnostic dumps, or generated app/build packages.
Use clearly labeled synthetic content for documentation screenshots. Review the staged
diff and file list so unrelated work or local artifacts are not included.

Preserve attribution and third-party notices. See [UPSTREAM-NOTICE.md](UPSTREAM-NOTICE.md)
for the available licensing materials and artwork provenance; do not replace that record
with an unsupported license or official-affiliation claim.
