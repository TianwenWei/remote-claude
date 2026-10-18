# Remote Claude：朋友试用说明

Remote Claude 是第三方 macOS 客户端，通过 SSH 操作你自己 Linux VPS 上的 Claude Code。项目文件和 Claude 登录保存在你的 VPS；连接配置和历史缓存保存在你的 Mac。每个人使用自己的服务器与账号。

## 1. 准备 VPS

- 准备可从 Mac 访问的 SSH 主机、端口和 Linux 用户；该用户需能写入自己的项目目录。
- 安装 Bash、Python 3 和 Claude Code；项目使用 Git 时也需要 Git。
- 用这个 Linux 用户运行 `claude`，按提示登录你自己的账号，并先确认命令行可正常使用。账号需具备 Claude Code 使用权限；免费 Claude 账号不包含该权限。登录方式见 [Claude Code 官方说明](https://code.claude.com/docs/en/setup)。
- VPS 需能访问 Claude 服务；如使用代理，在 VPS 的 Claude Code 环境配置它。

## 2. 准备 SSH 密钥

已有可用密钥时直接使用。需要新密钥时，在 Mac 终端运行：

```sh
ssh-keygen -t ed25519 -f "$HOME/.ssh/remote_claude"
```

将 `~/.ssh/remote_claude.pub` 的公钥添加到 VPS 上目标用户的 `~/.ssh/authorized_keys`，或使用云厂商的 SSH 公钥管理功能。私钥文件没有 `.pub` 后缀，保留在自己的 Mac。加密私钥可以先加载到系统 SSH agent：

```sh
ssh-add "$HOME/.ssh/remote_claude"
```

首次连接前，从云厂商控制台或已经可信的服务器会话取得 SSH 主机 SHA256 指纹。例如服务器使用 ED25519 主机密钥时，可以在服务器控制台运行：

```sh
ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub -E sha256
```

该指纹用于核验服务器身份，和用户登录公钥的指纹不同。

## 3. 安装和连接

1. Apple Silicon（M 系列）Mac 使用 `arm64` 包；Intel Mac 使用 `intel` 包。打开 DMG，将 `Remote Claude.app` 拖入 Applications。
2. 打开应用，进入「设置 → 工作区 → SSH 远程项目」，填写自己的主机、端口和用户。选择这台 Mac 上的 SSH 私钥；也可留空使用默认密钥或 SSH agent。
3. 检查服务器主机指纹，与第一步取得的可信指纹一致后显式确认。已知主机密钥发生变化时，应用拒绝继续；先查明是否重装了 VPS 或连错主机，再自行维护 known_hosts。
4. 连接并检查远程环境。项目目录默认 `~`，会解析成 SSH 用户的主目录。选择已有项目目录，或选择父目录并创建新项目。
5. 添加项目后，从侧边栏打开会话。聊天、文件、Git 和底部终端都操作该 VPS。

## 安装包签名

随包 JSON 清单记录架构、源码提交、SHA256、签名和公证状态。标有 `ad-hoc` / `notarized: false` 的是个人试用包，macOS 可能显示无法验证开发者或阻止打开。请核对来源和 SHA256，按系统提供的隐私与安全性提示自行决定是否允许打开；不要关闭 Gatekeeper 或运行移除安全检查的脚本。更顺畅的安装需要发布者使用自己的 Developer ID Application 证书签名并完成 Apple 公证。

## 数据和限制

- 本地数据目录：`~/.ccgui-remote-vps`；更新安装包会继续使用已有设置和缓存。
- 首次读取历史需要联网；已缓存历史在断网时仍可查看。
- VPS 登录凭据留在 VPS；应用不把 Mac 上的模型渠道密钥发送给 VPS。
- 5 小时/周额度来自远程已有 Claude OAuth 登录的查询。缺失或不支持时显示未知，不影响已缓存历史。
- 当前主要面向单个 VPS；不同主机使用相同或嵌套项目路径仍有限制。
- 首次连接检查主要支持直接 IP / DNS。使用 ProxyJump、ProxyCommand、HostName 重定向、主机证书或自定义密钥撤销策略的高级 SSH 配置可能被拒绝；请先使用兼容的直连配置，不要关闭主机校验。
- 这是朋友试用版，未完成朋友不同 VPS、Linux 发行版和 Intel Mac 的兼容性验收。

源码与构建说明见仓库的 `README.remote-vps.md`；上游与图标出处见 `UPSTREAM-NOTICE.md`。报告问题时附上应用版本、系统/芯片、错误阶段和错误文字，避免发送私钥、令牌或完整会话内容。
