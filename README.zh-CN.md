<img src="src-tauri/icons/remote-claude/icon.png" alt="Remote Claude" width="80" height="80">

# Remote Claude

[English](README.md) · **简体中文**

[VPS 配置](#先配置你的-vps) · [快速开始](#快速开始) · [源码构建](#从源码构建) · [贡献指南](CONTRIBUTING.md)

**在 Mac 上使用自己的 VPS，运行 Claude Code。**

Remote Claude 是一个 macOS 桌面客户端，通过 SSH 连接 Linux VPS，在本地窗口中管理远程项目、与 Claude Code 对话、查看文件和使用终端。

它适合希望把代码仓库、开发工具和 Claude Code 运行环境放在自己服务器上的开发者。**Mac 安装 Remote Claude，VPS 安装 Claude Code**；执行命令、编译、测试和修改文件都在 VPS 上完成，使用的是服务器上的环境。

你使用自己的服务器与 Claude 账号。Claude 登录保存在 VPS，界面和历史缓存保存在 Mac，客户端通过 SSH 直接连接服务器。

本项目基于 [CC GUI](https://github.com/zhukunpenglinyutong/desktop-cc-gui) 开发，专注于 Claude Code 的远程工作区体验，是独立的第三方客户端。

> 当前为预览版。已验证的安装包面向 Apple Silicon Mac，采用 ad-hoc 签名，尚未完成 Apple 公证。安装包由维护者单独提供；源码仓库不包含编译产物。

## 先配置你的 VPS

打开应用之前，先准备一个可以通过 SSH 登录、能够写入项目目录的 Linux 用户。**安装 Claude Code、登录 Claude 和客户端连接都使用同一个 Linux 用户**。VPS 应启用 SSH 服务，并在防火墙或云安全组中放通对应端口；VPS 本身也需要能访问 Claude 服务。

### 1. 在 VPS 上安装工具并登录

以下是 **Ubuntu/Debian 示例**，假设你已经可以登录 VPS。在 VPS 终端中，以之后用于客户端连接的用户执行；其他发行版使用对应的包管理器。只有安装系统软件包的步骤使用 `sudo`。

```sh
sudo apt-get update
sudo apt-get install -y bash python3 git curl ca-certificates util-linux
```

以准备使用的 Linux 用户，通过 [官方 Linux 安装方式](https://code.claude.com/docs/en/setup) 安装 Claude Code，然后登录：

```sh
curl -fsSL https://claude.ai/install.sh | bash
export PATH="$HOME/.local/bin:$PATH"
claude --version
claude auth login
claude auth status
mkdir -p "$HOME/projects"
```

还要确保 `~/.local/bin` 已加入该用户的 **登录 shell PATH**。上面的 `export` 只对当前终端生效；使用 Bash 时，将这一行加入该用户实际加载的登录配置，例如 `~/.profile` 或 `~/.bash_profile`。下面会通过一次新的 SSH 连接，检查客户端能否在非交互环境中找到 Claude Code。

服务器没有浏览器时，在 Mac 浏览器打开终端给出的登录链接，按提示完成授权；如果要求输入验证码，将验证码粘贴回 VPS 终端。具体流程见 [官方登录说明](https://code.claude.com/docs/en/authentication)。账号需要具备 Claude Code 使用权限；登录后在项目目录运行 `claude`，先确认终端中能够正常对话。

项目代码和项目所需的开发依赖也放在 VPS。可以把 `~/projects` 作为项目父目录，再通过客户端添加已有项目或创建新项目。客户端使用现有 SSH 服务，无需额外部署 Remote Claude 的 Web 服务。

### 2. 配置 Mac 到 VPS 的 SSH 密钥登录

将 Mac 的 **SSH 公钥**加入 VPS 对应用户的 `~/.ssh/authorized_keys`，或使用云厂商提供的 SSH 公钥配置功能。私钥保留在 Mac。生成密钥和核验服务器指纹的方法见 [首次连接指南](docs/remote-claude-trial.zh-CN.md)。

在一个 **新的 Mac 终端**验证密钥登录和远程登录环境，将下面的占位用户名、域名和端口替换为自己的配置；使用非默认私钥时，增加 `-i /path/to/private-key` 参数：

```sh
ssh -p 22 -o PreferredAuthentications=publickey your-user@your-vps.example.com 'bash -lc "command -v claude && claude auth status"'
```

首次接受服务器主机密钥前，通过可信渠道核对主机指纹。私钥有密码时，先用 `ssh-add` 加载到 Mac 的 SSH agent。检查应输出 Claude 可执行文件路径和已登录状态；找不到 `claude` 时，先修正远程用户的登录 shell PATH。检查通过后，继续[安装并连接客户端](#快速开始)。

## 主要功能

- **远程对话**：流式回复、工具调用展示、权限问答和任务停止。
- **项目管理**：添加 VPS 上的已有项目，或在选定父目录下创建新项目。
- **文件与 Git**：浏览、编辑、搜索远程文件，执行常用 Git 操作。
- **集成终端**：在窗口底部直接使用 VPS 的 SSH 终端。
- **历史缓存**：优先展示本地缓存，后台更新；断网时仍可查看已缓存会话。
- **用量展示**：分别查看上下文用量，以及支持该查询的 Claude 账号 5 小时和周额度。
- **首次连接引导**：选择本机私钥、核验服务器指纹、检查远端工具和 Claude 登录状态。

## 界面预览

以下截图来自客户端实际界面组件，项目、会话和终端输出均为演示数据。

**远程项目与对话**

![Remote Claude 的项目列表、会话与聊天界面](docs/screenshots/remote-chat.png)

**窗口底部的 SSH 终端**

![Remote Claude 的集成终端](docs/screenshots/remote-terminal.png)

**首次 SSH 连接配置**

![Remote Claude 的 SSH 主机、密钥与远程目录配置](docs/screenshots/ssh-setup.png)

## 工作方式

```text
Mac · Remote Claude
  聊天界面 / 连接设置 / 本地历史缓存
                │
          SSH 密钥认证
                │
Linux VPS
  Claude Code / 项目文件 / Git / Shell
                │
       用户自己的 Claude 账号
```

Mac 无需安装 Claude Code，VPS 无需部署额外的 Web 服务。客户端使用 SSH 密钥认证，并保留 OpenSSH 的主机身份校验。

## 使用要求

| 环境 | 要求 |
| --- | --- |
| Mac | macOS；当前已验证 Apple Silicon 安装包 |
| VPS | 可通过 SSH 访问的 Linux 主机，以及可访问项目目录的用户 |
| 必需工具 | VPS 上的 Bash、Python 3、`setsid`（Ubuntu/Debian 由 `util-linux` 提供）、Claude Code |
| 可选工具 | VPS 上的 Git，用于 Git 功能 |
| Claude 登录 | VPS 对应用户已登录具备 Claude Code 使用权限的账号；CLI 支持结构化登录状态查询 |
| SSH 认证 | Mac 上可用的 SSH 私钥、系统默认密钥或 SSH agent |
| 网络 | Mac 能访问 VPS 的 SSH 端口；VPS 能访问 Claude 服务 |

Claude Code 的安装与登录方式见 [官方说明](https://code.claude.com/docs/en/setup)。

## 快速开始

### 1. 准备 VPS 和 SSH

完成[开头的 VPS 配置](#先配置你的-vps)：SSH 密钥登录可用、远程工具齐全、Claude Code 已登录、项目目录可写。提前从云厂商控制台或已有的可信服务器会话取得 SSH 主机 SHA256 指纹，用于首次连接时核验身份。

### 2. 安装应用

取得与你的 Mac 芯片匹配的安装包及校验清单。打开 DMG，将 `Remote Claude.app` 拖入 `Applications`，然后启动应用。

当前提供的预览包未经 Apple 公证，macOS 可能提示无法验证开发者。请核对包的来源与 SHA256，并按系统提供的安全提示处理。正式签名与公证的构建入口见下方说明。

### 3. 添加远程项目

打开 **设置 → 工作区 → SSH 远程项目**，填写连接信息：

| 配置项 | 说明 |
| --- | --- |
| SSH 主机 | VPS 的 IP 地址或 DNS 名称 |
| SSH 用户 | VPS 上已准备好 Claude Code 和项目权限的 Linux 用户 |
| SSH 端口 | 默认 `22` |
| 私钥 | 用文件选择器选择这台 Mac 上的私钥；留空使用默认密钥或 SSH agent |
| 项目目录 | 默认 `~`，表示远程用户主目录；连接后可浏览并选择项目文件夹 |

点击 **连接并浏览目录**。首次接入新主机时，核对显示的指纹与可信渠道取得的指纹，确认一致后显式信任。应用随后检查工具和 Claude 登录状态；准备就绪后，选择项目目录并点击 **添加此远程项目**。

主机密钥发生变化时，应用会阻止连接，不会自动替换原有信任记录。

### 4. 开始使用

从侧边栏打开项目并创建会话。聊天、文件操作、Git 和底部终端均作用于该 VPS。

需要新项目时，点击侧边栏的 **新建远程项目**，选择 VPS 上的父目录，填写项目名称，再点击 **创建并添加项目**。同名目录不会被覆盖。

## 数据与凭据

- **本地数据目录**：`~/.ccgui-remote-vps`，保存应用设置、工作区信息和历史缓存。
- **应用身份**：Bundle ID 为 `local.ccgui.remote-vps`，与上游 CC GUI 的数据目录分开。
- **SSH 私钥**：客户端保存文件路径，密钥认证由本机 SSH 完成；私钥不会复制到 VPS。
- **Claude 登录**：由 VPS 上的 Claude Code 管理。账号额度查询也在 VPS 执行，OAuth 令牌不传回 Mac。
- **模型配置**：远程项目使用 VPS 自身的 Claude 配置，不发送 Mac 上配置的模型渠道密钥。
- **离线历史**：已缓存的会话可离线查看；首次读取、获取新消息和执行操作需要联网。

上下文用量与账号额度是两个独立指标。额度查询依赖 Claude 内部接口，字段或认证方式可能变化；无法取得数据时显示未知，不推算剩余额度。

## 从源码构建

需要 macOS、Xcode Command Line Tools、Node.js、pnpm 和 Rust stable。项目使用 `pnpm@10.29.3`；制作分发包还需要 Python 3。

```sh
git clone git@github.com:TianwenWei/remote-claude.git
cd remote-claude
pnpm install --frozen-lockfile
./scripts/build-remote-macos.sh
```

私有仓库需要相应的 GitHub 访问权限。使用源码 ZIP 时，解压后从安装依赖这一步开始即可。

默认 Release 产物：

```text
src-tauri/target/release/bundle/macos/Remote Claude.app
```

开发模式：

```sh
pnpm exec tauri dev --features remote-vps --config src-tauri/tauri.remote.conf.json
```

生成开发包：

```sh
./scripts/build-remote-macos.sh --debug
```

Remote 构建必须同时使用 `remote-vps` Cargo feature 与 `tauri.remote.conf.json`，以保留独立的应用身份、数据目录和更新配置。上面的构建脚本已经包含这两个参数。

### 制作安装包

```sh
python3 scripts/package-remote-macos.py --ad-hoc
```

默认生成 Apple Silicon 预览包，输出到 `artifacts/remote-claude-trial/`，包括 DMG、App ZIP、构建清单和 SHA256。清单记录源码提交、架构、签名和公证状态。

脚本也提供 Intel、Universal 以及自有 Developer ID 签名和公证参数。这些构建选项不代表对应平台或签名流程已完成验收。完整参数与操作说明见 [Remote 开发与构建说明](README.remote-vps.md)。

## 开发验证

常用本地检查：

```sh
pnpm build
pnpm exec vitest run src/features/settings/SshWorkspacesCard.test.tsx
cargo test --manifest-path src-tauri/Cargo.toml --features remote-vps --lib engine::ssh_onboarding::tests
node --test tests/xterm-production.test.mjs
```

生产终端回归需要先执行 `pnpm build`。真实 SSH 与模型测试需要显式配置已授权的测试主机，详见 [Remote 开发与构建说明](README.remote-vps.md)。

现有验证以维护者使用的 Apple Silicon Mac 和已授权 VPS 为主。尚未完成不同用户 VPS、Intel Mac 和正式签名分发的兼容性验收；本地检查通过也不代表真实 SSH 链路已验证。

## 当前限制

- 当前 Remote 版本面向 macOS 与 Linux VPS；不承诺上游所有引擎或平台的功能均适用于 SSH 工作区。
- 不支持远程 Worktree 管理、本机电脑操作或本机记忆工具；Worktree 可在远程终端中管理。
- 不同 SSH 主机使用相同或嵌套项目路径时，工作区注册仍有限制。
- 首次连接检查对 ProxyJump、ProxyCommand、HostName 重定向、主机证书和自定义撤销策略等高级 SSH 配置有限制。
- 旧版 Claude Code 无法提供可确认的登录状态时，会阻止首次注册并显示原因。
- 上游自动更新已禁用；更新应用需手动替换安装包，已有设置和缓存继续保留。

## 常见问题

| 情况 | 处理方式 |
| --- | --- |
| SSH 超时或连接被关闭 | 检查 Mac 到 VPS 的网络、SSH 服务和认证；已缓存的历史仍可查看 |
| 服务器指纹与记录冲突 | 通过服务器控制台核实主机身份及是否重装，再维护本机 OpenSSH 信任记录 |
| 找不到 Claude Code 或未登录 | 在 VPS 对应用户的登录环境中安装、登录并检查 Claude Code |
| 缺少 Git | 聊天项目仍可使用；安装 Git 后再使用 Git 功能 |
| 账号额度显示未知 | 确认远端登录方式支持该查询；接口不可用时应用不会推算额度 |

## 反馈与贡献

欢迎提交可复现的问题、文档修正和小范围 PR。请使用仓库的 [问题反馈模板](.github/ISSUE_TEMPLATE/bug_report.yml)，提供版本、Mac 芯片与系统、复现步骤、错误阶段及去敏后的错误文字。不要附上私钥、访问令牌或完整会话内容。

开发流程、相关检查和文档同步要求见 [贡献指南](CONTRIBUTING.md)。更详细的实现与验证说明见 [Remote 开发与构建说明](README.remote-vps.md)。中英文 README 保持同步。

## 项目来源与许可材料

Remote Claude 基于 [zhukunpenglinyutong/desktop-cc-gui](https://github.com/zhukunpenglinyutong/desktop-cc-gui) 的 1.1.4 源码开发。感谢原项目及其贡献者。

当前采用的上游源码快照缺少根目录 LICENSE 文件。现有许可材料、历史出处与 Claude 图标来源记录在 [UPSTREAM-NOTICE.md](UPSTREAM-NOTICE.md)，扩大分发前需核实现有材料。Claude 名称和图标属于相应权利人，本项目不是 Anthropic 官方产品。
