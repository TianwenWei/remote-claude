# Remote Claude（Mac → Linux VPS）

Remote Claude（原名 CC GUI Remote）基于 CC GUI 1.1.4，增加原生 Linux SSH 工作区。Mac 运行本地界面，VPS 运行 Claude Code 并保存项目文件；无需在 Mac 安装 Claude Code。它直接使用 SSH，不依赖 cdesktop 的网页或接口。

本仓库提供 macOS 朋友试用版。首次使用请阅读 [朋友试用说明](docs/remote-claude-trial.zh-CN.md)，上游许可与图标出处见 [UPSTREAM-NOTICE.md](UPSTREAM-NOTICE.md)。仓库地址：<https://github.com/TianwenWei/remote-claude>。

## 使用

打开 **Remote Claude**，进入「设置 → 工作区 → SSH 远程项目」。填写主机、用户、端口、Mac 上的 SSH 私钥路径和 VPS 项目目录，点击「连接并浏览目录」，选择项目文件夹，再点击「添加此远程项目」。侧边栏中会显示 SSH 主机。

首次配置的主机、用户和私钥为空；端口默认 22，项目目录默认 `~`（远程用户主目录）。私钥可以用文件选择器选择绝对路径，留空时使用系统默认密钥或 SSH agent。已有工作区的连接设置继续沿用，不替换为示例地址。

首次接入新主机时显示算法与 SHA256 主机指纹；从服务器控制台等可信渠道核对后，勾选确认并显式信任。应用会重新读取主机密钥并比对所选指纹，再追加到实际使用的用户 known_hosts 文件；已知主机密钥变化时拒绝覆盖或继续。连接探针显示 Bash、Python、Git、Claude Code 与登录状态，不调用模型、不传出登录令牌；缺少必要工具、未登录或登录状态未知时显示原因，准备完成后才能添加或创建项目。Git 为可选依赖，缺少时提示 Git 功能不可用，不阻止 Claude 聊天项目。

创建新项目时，点击侧边栏的「新建远程项目」，连接并选择 VPS 上的父目录，填写「新项目名称」，再点击「创建并添加项目」。应用会创建新文件夹并立即打开远程对话；同名文件或目录会报错，不会覆盖。连接信息默认沿用当前 SSH 项目。

远程主机需要 Bash、Python 3 和已登录的 Claude Code；Git 功能需要 Git。SSH 使用密钥和已有的 known_hosts 记录，连接失败时会显示错误。应用不会读取并发送 Mac 上配置的模型渠道密钥。

支持远程 Claude 对话流、权限问答、停止任务、历史回放，以及远程文件浏览/编辑、搜索、常用 Git 操作和集成终端。Worktree 管理和 Mac 操作电脑/记忆工具不支持远程项目；可在远程终端管理 Worktree。

历史会完整缓存在这台 Mac 的 `~/.ccgui-remote-vps/remote-sessions`。再次打开时先显示本地缓存，再后台检查服务器更新；离线或更新失败时仍可查看已缓存的历史和向前翻页。界面会标明缓存和更新状态；首次打开尚未缓存的会话仍需要连接服务器。会话目录也会保存在本地，以便重启后离线打开已缓存的会话。

后台 SSH 查询按主机和端口共享队列，最多同时执行 3 个；完整历史下载最多占 1 个位置，保留其他查询的执行机会。启动远程 Claude 前的 Linux 临时脚本上传也使用同一队列，上传结束即释放位置，Claude 长任务本身不占用短调用位置。Mac 上的会话、文件、Git 状态、模型与额度等只读查询复用 SSH 连接，连接空闲 60 秒后关闭；不同用户或私钥配置分别复用。重复的同项目会话目录刷新共用一个请求，减少启动或切换会话时的握手。排队最多等待 60 秒，与开始执行后的 30 秒超时分开计算（完整历史下载为 300 秒）；取消、失败或结束均释放位置。脚本上传的写入与 SSH 退出等待共用一个 30 秒期限，并持续读取 SSH 错误输出，保留前 16 KiB 后仍继续排空，避免管道阻塞；超时终止该上传进程，显示失败阶段及已取得的 SSH 诊断，不自动重试。文件与 Git 写操作、远程 Claude 长任务和交互终端使用独立连接；WSL 不加入该主机队列，仍采用原有 `tee` / `wsl.exe` 上传路径，共用上述上传超时和错误读取处理。

底部上下文仪表使用最近一次真实模型请求的用量；额度耗尽等合成错误消息不会把它清成零，未取得用量时显示 `—`。Claude 的 5 小时与周额度另以「余 5h / 周」显示，点击可查看账号共享的剩余比例、重置时间与查询时间。首次打开、任务结束及打开额度详情时按需查询，60 秒内复用数据，不持续轮询；可手动刷新，失败保留上次数据并等待 30 秒后重试。

额度查询由 VPS 内的 Python 使用该主机现有 Claude OAuth 登录读取 `https://api.anthropic.com/api/oauth/usage`，令牌不传到 Mac，HTTPS 不跟随重定向。它是 Claude 内部接口，字段或认证策略可能变化；缺少数据时显示未知，不推算额度。仅支持已注册的 Linux SSH 工作区。

## 独立安装与数据

- macOS 应用名称：`Remote Claude`。
- Bundle ID：`local.ccgui.remote-vps`。
- 数据目录：`~/.ccgui-remote-vps`。
- 与现有 `CC GUI.app`、`~/.ccgui-next` 分开，首次启动不导入旧应用数据。
- 禁用上游自动更新，避免被官方构建覆盖。这个本机构建使用 ad-hoc 签名。
- Remote 专用 Claude 图标位于 `src-tauri/icons/remote-claude/`：`icon.png`、`icon.icns`、`icon.ico`、`32x32.png`、`128x128.png`、`128x128@2x.png`。素材与 iOS 一样来自已缓存的官方 App Store 图标，仅 Remote 配置引用这些文件，上游图标保持原样。

此次改名沿用原 Bundle ID 和数据目录，保留既有工作区、设置与历史缓存。

## 构建

需要 Xcode Command Line Tools、Node.js、pnpm 和 Rust stable。

```sh
pnpm install --frozen-lockfile
./scripts/build-remote-macos.sh
# 开发构建：
./scripts/build-remote-macos.sh --debug
```

产物在 `src-tauri/target/release/bundle/macos/Remote Claude.app`；开发构建为 `src-tauri/target/debug/bundle/macos/Remote Claude.app`。`remote-vps` feature 决定独立数据目录，必须与远程配置文件一起使用。

朋友试用包使用 release 构建，并输出 DMG、App ZIP、签名/架构/源码提交清单和 SHA256。明确选择个人试用签名，或配置自己的 Developer ID；不读取上游发布者的证书或更新密钥：

```sh
# Apple Silicon 个人试用包（未经公证）
python3 scripts/package-remote-macos.py --ad-hoc
# Intel 版本，先安装相应 Rust target
rustup target add x86_64-apple-darwin
python3 scripts/package-remote-macos.py --ad-hoc --target x86_64-apple-darwin --output artifacts/remote-claude-intel
# 正式签名和公证；NOTARY_PROFILE 是本机已有 Keychain profile 名称
python3 scripts/package-remote-macos.py \
  --signing-identity 'Developer ID Application: YOUR NAME (TEAM_ID)' \
  --notary-profile NOTARY_PROFILE --output artifacts/remote-claude-signed
```

默认输出 `artifacts/remote-claude-trial/`，已被 Git 忽略。发布到仓库的源码不包含 `.ssh`、应用数据、历史缓存、构建产物或本机连接配置。当前试用构建沿用版本 1.1.4，通过清单中的 `channel: friends-trial` 与源码提交区分；上游自动更新保持禁用。

终端回归测试需要先生成生产前端产物，再执行：

```sh
npm run build
node --test tests/xterm-production.test.mjs
```

该测试直接运行打包后的 xterm，验证 ANSI/DEC 模式查询、状态切换、分片输入及后续文本解析，不连接 SSH，也不调用模型。Vite 对 xterm 使用精确的 CJS 入口别名，避开 xterm 6 ESM 在二次压缩时产生的未声明变量错误。

只读连接复用测试使用真实 SSH，不调用模型或修改远程文件。它测试 3 个冷启动请求及 10 个后续请求，并关闭该测试创建的连接；已有复用连接时拒绝接管。需显式设置已获授权的连接参数：

```bash
CCGUI_SSH_LIVE_HOST=example.com CCGUI_SSH_LIVE_USER=cc \
CCGUI_SSH_LIVE_KEY=/absolute/path/to/key \
cargo test --manifest-path src-tauri/Cargo.toml --features remote-vps --lib ssh_readonly_mux_cold_and_warm_batches -- --ignored --nocapture
```

完整 SSH 验证测试需显式设置已获授权的连接参数：

```sh
CCGUI_SSH_LIVE_HOST=example.com CCGUI_SSH_LIVE_USER=cc \
CCGUI_SSH_LIVE_KEY=/absolute/path/to/key \
CCGUI_SSH_LIVE_WORKSPACE=/home/cc/projects/example \
cargo test --manifest-path src-tauri/Cargo.toml --features remote-vps --test ssh_live -- --ignored --nocapture
```

该测试会调用真实 Claude Code，验证流式回复、写文件审批、远程文件读取/回收站、历史回放和 SSH 终端。
