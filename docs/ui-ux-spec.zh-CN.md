# CC GUI 界面与交互规范（UI/UX Spec）

> **版本**：v0.1（首版，随实现增量维护）
> **适用范围**：`src/styles`、`src/components`、`src/features` 中所有用户可见的界面与交互
> **关联代码**：`src/styles/theme.css`（设计 token）、`src/styles/typography.css`（文本样式）、`src/components/base/*`（基础组件）、`src/i18n/{zh,en}.ts`（文案）

这是本项目界面与交互的**唯一约定来源**。凡是"两个地方长得不一样、动得不一样"的问题，先在这里定一条规则，再改代码；代码里出现的做法如果值得复用，就回写成规则。

文档只写**已经落地、能指到代码**的规则；暂时做不到的放进 [§8 待收敛](#8-待收敛)，不假装成立。

---

## 目录

1. [维护约定](#1-维护约定)
2. [设计基础](#2-设计基础)
3. [状态与可用性](#3-状态与可用性)
4. [动作反馈](#4-动作反馈)
5. [加载、空状态与错误](#5-加载空状态与错误)
6. [破坏性操作](#6-破坏性操作)
7. [刷新入口清单](#7-刷新入口清单)
8. [待收敛](#8-待收敛)

---

## 1. 维护约定

- **谁改谁更新**：新增或修改用户可见的交互反馈、基础组件视觉、动效时长时，在同一个提交里更新本文对应章节。
- **新增刷新/重载入口必须登记**到 [§7 刷新入口清单](#7-刷新入口清单)；没登记的算没做完。
- **规则要能落到代码**：每条规则写清文件和常量名。文档与代码冲突时**以代码为准**，并立即修正文档（或反过来改代码 + 补充说明）。
- 只维护这一份中文文档，不再派生第二份双语副本，避免两边漂移。
- 规则来自真实问题，不预先发明：同一条规则第二次被违反时，才值得写进来。

## 2. 设计基础

- **字体高级设置**：设置 → 通用 → 外观的「高级」区域默认收起，展开/收起复用 `Collapsible`（200ms）。提供 px 数值输入「界面字号 / 内容字体大小 / 代码字体大小」（10–32px，整数，默认 16 / 14 / 13px），以及「界面字重」（标准 / 稍粗 / 加粗，默认标准）。数值在失焦或 Enter 时应用并保存；空值、越界与非整数恢复当前值。「重置为默认设置」一次恢复这四项，不重置字体文件、主题或界面缩放。界面字号按比例调整文字与行高，保留层级，不改变图标或布局间距；内容字号独立作用于聊天正文，代码字号作用于代码块、编辑器与差异视图，终端保持独立。字重在原值上增加 0 / 100 / 200（最高 900），代码区域不跟随界面加粗。`uiFontSize` / `contentFontSize` / `codeFontSize` / `uiFontWeight` 持久化到 AppSettings，localStorage 镜像在首帧前应用；保存失败恢复原设置并提示错误。搜索这些设置自动展开「高级」。实现：`interface-typography.ts`、`typography.css`、`GeneralSection.tsx`。

### 2.1 Token 优先

- 颜色、圆角、阴影、动效一律引用 `src/styles/theme.css` 的**语义 token**（`text-*`、`background-*`、`border-*`、`foreground-icon-*`、`notification-*`、`shadow-*`），不写死色值、不直接引用 `slate-*` 之类的原始色阶。
- 文本排版用 `src/styles/typography.css` 的 `text-*` 系列（`text-body-medium`、`text-caption-1-regular`…）。`cx()` 依赖 `src/utils/cx.ts` 里登记过的文本族，新增文本族必须同步登记，否则会被 tailwind-merge 当成颜色丢掉。
- 关键帧动画定义在 `src/styles/theme.css` 的 `@theme` 里（如 `--animate-refresh-spin`）并配 `@keyframes`，组件只引用动画名。

### 2.2 组件优先

- 交互控件优先复用 `src/components/base/*`（`Button`、`IconButton`、`Dropdown`、`Select`、`Input`、`Switch`、`Tooltip`…）。页面内自造按钮要么说明 base 组件为什么不适用，要么把它沉淀成 base 组件。
- 尺寸以组件自身定义为准，不在调用点临时改高度：`Button` medium 36 / small 32 / xs 24，`IconButton` medium 36 / small 32。
- `Button` / `IconButton` 需要承载动态图标时用 `children` 覆盖默认图标（两者同一契约），不要用 `[&_svg]:animate-spin` 这类穿透选择器改图标行为。
- 图标一律取 `lucide-react/dist/esm/icons/*`（按需具名导入）；不新引图标库，也不自绘 SVG。同一入口出现在多个位置时三处共用同一个图标：插件/插件市场 = `layout-grid`（侧边栏 `sidebar-chrome.tsx`、插件中心页签 `use-chat-tabs.ts`、设置页插件条目无品牌素材时的兜底 `PluginSettingsNavIcon.tsx`）。

### 2.3 文案与无障碍

- 用户可见文案一律从 `src/i18n/zh.ts`、`src/i18n/en.ts` 取，两个语言文件同步新增 key，组件里不写死中文。
- 图标按钮必须同时有 `aria-label`（可访问名）和 `title`（指针悬停）。可访问名用**动作名**（"刷新"、"重新加载"），不用"点这里"。
- 需要解释性文案、快捷键或多行说明时才用 `Tooltip`（`src/components/base/tooltip/tooltip.tsx`）：它基于 react-aria，trigger 必须是 react-aria 组件或包在 `Focusable` 里的元素；触屏上不可达，所以**关键信息不能只放在 tooltip 里**。
- 原生 `title` 提示由 `NativeTitleTooltip`（`src/components/base/tooltip/native-title-tooltip.tsx`，挂在 `App.tsx`）全局接管：它监听 `document.body`（覆盖 portal 到 body 的右键菜单、对话框），把 `title` 文案搬进 `data-native-tooltip` 并置空原属性，改渲染与 `TooltipContent` 同一视觉的气泡（500ms 延迟、150ms 进入过渡、`z-[130]`，高于对话框 z-110 / 右键菜单 z-120），同时把文案复制为 `aria-description` 保住读屏描述。写控件时仍直接写 `title` 即可，不要手动操作 `data-native-tooltip`；Esc / 滚动 / 按下 / 目标卸载都会收起。

### 2.4 展开 / 收起动效

- **把下面内容推开的展开/收起一律做高度过渡，不允许条件渲染直接闪现**（`{open && …}` 少了过渡就是瞬间抽走 / 塞回）。实现统一用 CSS `grid-template-rows: 1fr ⇄ 0fr`，让浏览器插值轨道高度、兄弟行被平滑推开；不用 `max-height` 猜高度，也不用 JS 逐帧量高。现成实现两处，不要另起：侧栏树/列表用 `src/components/application/ai-chat/repo-tree.tsx` 的 `SidebarDisclosure`（`WorktreeGroup` 分组与线程列表共用：`300ms` + `ease-in-out` + `motion-reduce:transition-none`）；消息/面板级折叠用 `src/components/application/collapsible/collapsible.tsx` 的 `Collapsible`（额外 `opacity` + `visibility` 收尾，时长由调用点给）。两者都只转箭头方向（`transition-transform duration-150` + 展开时 `rotate-90`）并同步 `aria-expanded`。
- **收起过程中内容保持挂载，动画结束才卸载**：内容要在关闭动画期间留在 DOM 里（`SIDEBAR_COLLAPSE_MS` = 300ms 后卸载，顺带回收分页等组件内状态，下次展开回到第一页），否则收起读起来是「啪」地消失。
- **不用 `opacity` 淡出列表行**：列表淡出会在下方内容上留一帧合成残影（`repo-tree.tsx` 的注释记录了这次事故）；只裁切高度。收起期间给区域 `aria-hidden="true"` + `inert`，键盘与读屏不进入被裁掉的内容。长消息的「展开全文」（`CollapsibleMessage`，`max-height` + 渐变遮罩）是把气泡裁短而不是推动兄弟行，属于另一类视觉，不在这条规则内。
- **回归**：`tests/browser/sidebar-collapse.html` 逐帧采样「WORKTREES」分组与 worktree 子行的展开/收起高度——每个方向必须经过 ≥3 个中间帧、收起期间行仍在 DOM（只有首尾两个值＝直接跳变）；`ai-chat-sidebar.test.tsx` 覆盖收起后 300ms 才卸载。

## 3. 状态与可用性

- **macOS Remote 品牌**：`remote-vps` 构建的应用显示名和主窗口标题为 `Remote Claude`；系统应用菜单与关于信息取 Tauri 包名，设置中的应用名通过原生 `getName()` 读取，Web / 读取失败时保留默认 `CC GUI`。Claude 图标仅由 Remote 配置引用 `src-tauri/icons/remote-claude/` 中的专用资源，正式产物为 `src-tauri/target/release/bundle/macos/Remote Claude.app`。Bundle ID `local.ccgui.remote-vps`、数据目录 `~/.ccgui-remote-vps` 与更新禁用配置保持不变；上游默认 `CC GUI` 品牌保持原样。实现：`src-tauri/tauri.remote.conf.json`、`src-tauri/src/lib.rs`、`src-tauri/src/app_menu.rs`、`src/features/settings/UpdateSection.tsx`。
- **macOS Remote 首次连接**：新用户的主机、用户名、私钥初始为空，端口 22、远程目录 `~`；已有 SSH 工作区继续沿用连接参数。私钥选择使用系统文件选择器，不读取或显示私钥内容。新主机显示算法和 SHA256 指纹，用户从其他可信渠道核验并勾选后才能显式信任；信任前重新检查，变化的主机密钥不得自动覆盖。连接探针展示依赖及 Claude 登录状态，缺少必要工具、未登录与未知状态分别可见；Bash/Python/Claude 与登录准备完成后才能添加/创建项目，Git 缺失只提示可选功能不可用。异步检查失效或组件卸载不得使用晚到结果授权新地址。实现：`src/features/settings/SshWorkspacesCard.tsx`、`src-tauri/src/engine/ssh_onboarding.rs`、`src-tauri/src/engine/remote_workspace.rs`。

- **远程工具逐次审批**：原生 `tool_permission` 事件在现有问答 dock 中显示独立工具权限卡（`ToolPermissionCard.tsx`）。与模型 `AskUserQuestion` 区分，标题「远程工具权限」、实际工具名与输入 JSON（纯文本、限高滚动；原生端最多显示 65,536 字符，截断时明确提示）供核对；仅有「仅允许本次 / 拒绝 / 跳过并拒绝」三个显式动作，无自由文本输入，也不默认聚焦批准。提交协议固定为 `{"__ccgui_tool_permission":"allow_once"}` / `"deny"`，跳过传 `null` 并拒绝，翻译只影响显示。提交中锁住所有动作防重入；发送失败保留待批请求和错误供重试，原生取消 / EOF 后失效，已结束记录只读且保留所核对输入。普通问答输入不能伪装成权限请求。实现：`engine-events.ts`、`store/messaging.ts`、`lib/tool-permission.ts`；原生端独立校验原工具上下文，不从模型提问内容推断许可。

- **原生 SSH 远程项目**：设置 → 工作区顶部提供「SSH 远程项目」卡（`SshWorkspacesCard.tsx`，仅桌面端）。主机、用户、端口、本机私钥路径与远程目录均可编辑；私钥路径允许留空，使用系统 SSH agent / 默认身份。先「连接并浏览目录」，显示 SSH / Claude Code 实际探测结果与可进入的子目录、上级入口，再「添加此远程项目」。输入变化立即废弃旧探测结果，连接失败清除旧目录和添加资格；未检测到远程 Claude 时给原因并禁止添加。请求中禁用字段与动作，重复点击不得并发注册；失败保留填写内容并显示 `role="alert"`，成功显示 `role="status"`。侧栏远程项目把 `SSH · 主机` 放在项目名下方的次要文字行，两行各自省略并提供完整悬停提示，窄侧栏中主机不得挤占项目名宽度；归档行沿用该布局，插件徽标保留行内显示。新会话选远程 Claude，引擎菜单按远程可用路径显示，不依赖本机 CLI。远程历史刷新失败保留已加载会话并显示具体错误，不用空列表冒充成功。实现：`ipc.ts`、`ChatConversation.tsx`、`use-chat-sidebar.ts`、`repo-tree.tsx`、`workspace-sections.tsx`、`store/ssh-sessions.ts`。

- **本地窗口新建远程项目**：侧栏项目树顶部直接显示「新建远程项目」，打开设置 → 工作区的 SSH 卡；沿用当前远程项目的连接信息，并以它的父目录作为初始位置，无远程项目时使用默认连接。连接并浏览后，所选现有目录作为父目录；「新项目名称」只接受单个目录名，不能是空白、`.`、`..` 或含路径分隔符或控制字符。展示将创建的完整远程路径，「创建并添加项目」调用 `create_ssh_workspace(connection,parentPath,name)`，原生端拒绝覆盖现有子目录。创建中禁用所有连接、浏览、注册与名称输入并防重复提交；失败保留名称与父目录、显示具体错误；成功注册后直接打开新项目的远程 Claude 对话。已创建但刷新失败时必须明确说明目录已创建，不声称创建失败。实现：`SshWorkspacesCard.tsx`、`WorkspacesSection.tsx`、`workspace-sections.tsx`、`use-chat-sidebar.ts`、`ipc.ts`。

- **插件会话模式**：获得 `ui:conversation-mode` 授权后，在 CLI 选择器旁提供入口，不伪装成 CLI 或权限选项；普通轮次或发送队列未结束时禁止切入。模式替换当前聊天内容区与输入框，保留原普通对话。运行、取消待确认及恢复待核对期间禁止通过宿主「返回普通对话」绕过插件的退出锁。
- **接力首版**：规划与执行分别配置引擎 / 渠道 / 模型，配置浮层限高滚动且动作区保持可见。允许多轮讨论和手工编辑，每份完整计划保存为新版本；只有最新、已保存、无未解问题且无未发送草稿的计划可点击「确认此版并执行」。普通发送、AI 回复结束和保存编辑均不授权执行。确认冻结交接包，执行新建原生会话；历史版本只读。
- **接力停止与恢复**：停止是请求取消，不是回滚；等待进程终态才结束忙碌状态，失败不自动重试写入。重新打开已保存接力记录先核对上次进程及工作区，不自动续跑。明确披露记录独立于普通聊天、其他会话与外部编辑器不受接力单任务锁保护。
- **活动插件标签保护**：插件报告忙碌后，标签关闭和会替换草稿的引擎切换同样受阻断；切换到别的标签不释放原任务锁。锁与草稿身份持久化，插件被禁用时显示恢复提示，不回落为可发送的普通对话；重新启用插件并停止 / 核对后解锁。目录读取失败仍显示恢复与停止入口。
- **AskUserQuestion 多题流转**：单选选项使用圆形单选标记，多选选项使用方形复选标记。单选答完自动跳到下一道未答题；中间的多选题不显示「提交」，须先按「确认本题并继续」保存本题选择；仅最后一道多选题显示「提交」，全部题目完成后统一提交。
- **计划预览与人工审批**：「计划」是输入框权限模式的一档，只有后端声明支持的引擎可选（`EngineInfo.permissions` 含 `plan`，其余置灰）；能力缺失的引擎在发送前受控拒绝并给出原因，不静默降级执行。规划期间时间线显示计划卡（标题 / 引擎 / 版本号 / 状态徽标 / 摘要），草稿只可流式预览不可批准；完整计划到达后进入「等待审批」。「查看完整计划」打开预览层（宽屏侧面板、窄屏全宽，复用消息 Markdown 渲染与安全处理），Esc / 点背景 / 关闭按钮只收起界面，不发送任何审批 IPC；复制按钮复制原文。审批操作在输入区 dock（复用提问 dock 布局与焦点模式）：「批准并执行」旁标注将沿用的执行权限（批准不改变权限），「提出修改」要求非空反馈（Enter 只换行，Cmd/Ctrl+Enter 提交），「暂不执行」结束本轮审批——dock 收起即点击的可见效果，计划保留未批准、原生等待点仍停着，时间线卡片显示「已暂不执行」提示与「继续审批」按钮可随时重开 dock；默认焦点不在批准按钮，提交中防重入（后端按 planId + expectedRevision CAS 仲裁，双击 / 双窗口只生效一次）。冲突（已被另一窗口或新版本取代）展示通知并保持只读快照面板；错误保持待审可重试，不伪成功。九个状态（草稿 / 等待审批 / 提交中 / 已批准 / 已请求修改 / 暂不执行 / 已取消 / 已失效 / 已被取代）各有不依赖颜色的可访问文本徽标；已批准 / 已替代 / 已失效版本只有查看与复制，不再有批准按钮。与插件会话模式双向互斥：插件模式激活时计划 dock 不挂载，计划等待且运行活跃时禁止切入插件模式。重开会话经 `listPlanReviews` 恢复历史计划记录与决策，前端 localStorage 不是审批权威。回归：`plan-review.test.tsx`、`tests/browser/plan-review.html`。

- **电脑操控逐次开启，不做全局开关**：`/ccgui-cua <任务>`（`app-commands.ts` 的 `parseAppCommand`）只为那一次发送挂载驱动（`send` 的 `SendOptions.computerUse`），排队消息带着同一标志（`QueuedMessage.computerUse`）——机器输入被预授权，不能从一条普通消息间接触达。设置页 `ComputerUseSection.tsx` 只给权限状态与授权引导，不放 on/off 开关。
- **引擎不支持电脑操控时必须明说**：`engineSupportsComputerUse` 读引擎能力位（`EngineInfo.supportsComputerUse`），为假时发送前拒绝并在会话错误条给出原因，不静默降级成普通对话；设置页按引擎列出支持情况（判定用精确文案，不能按“支持”子串误判“不支持”）。
- **权限行读真实系统状态**：`computer_use_permission_status` 决定已授权/未授权，`osPermissionsRequired` 为假（Windows/Linux）时显示“无需额外授权”而非未授权行，不追一条系统从不要的授权；macOS 的授权入口是「打开系统设置」按钮深链到对应面板（`computer_use_open_permission_settings`），页面不提供拖拽 App 图标的引导，也不暗示点一下就能自动授予。
- **虚拟光标由 App 强制显示，不是设置项**：运行期间 `cu_overlay.rs` 跟随每个动作目标显示指针，模型侧没有可关闭它的工具；提示词只能说明它存在（computer_use.rs 的 MCP `instructions`），不能决定其可见性。
- **急停只在电脑操控回合期间武装**：`computerUseSetActive` 在发送时武装、回合终止（`engine-events.ts` 的 `done`/`error`）时解除，全局 Esc 不超出它的运行期。
- **排队行可上下调序**：排队卡片每行在发送 / 移除之外给「上移 / 下移」箭头（`message-queue.tsx` 的 `onMove` → store 的 `moveQueued(id, "up" | "down")`），仅在队列多于一行时渲染；首尾行各有一个方向禁用（`disabled:cursor-default` + 降透明，不隐藏，控件不换位）。箭头按用户看到的列表方向移动——卡片是「最新在上、队首在下」，所以上移 = 更晚发送、下移 = 更早发送（`moveQueued` 里 `up` 即数组后移一位；越界与未知 id 为 no-op），行首编号随重排实时重算。回归：`message-queue.test.tsx`、`queue-drain.test.ts`。
- 可交互元素至少实现：默认 / hover / `focus-visible`（`ring-border-focus-ring`）/ active / disabled。按钮类控件的焦点环只走 `focus-visible`（不打扰鼠标用户）；输入类控件可以用 `focus:border-border-focus-ring` 表示聚焦，因为文本输入聚焦本身就是用户意图。三个搜索面板（⌘K 命令 / ⌘L 会话 / ⌘P 文件）的输入框例外：无边框，聚焦只靠光标与键盘高亮行，`.palette-search-field`（`globals.css`）负责压掉平台默认焦点框——Windows WebView2 会在 `outline-none` 之外再画一圈，macOS WKWebView 不画。
- **渠道选择后保留当前引擎面板**：`engine-model-panel.tsx` 的 `ChannelPicker` 在选项卸载前将焦点交回同一面板的渠道按钮（`preventScroll: true`），桌面浮层与移动端弹窗共用。不能让选项卸载后的焦点恢复落到首个引擎行，触发 `onFocus` 把 Codex 面板切成 Claude Code；正常的引擎行点击、键盘导航与悬停切换保持不变。
- **用量与消息记录按本轮实际模型归属**：`src/features/chat/store/engine-events.ts` 的 `stampedModel` 优先使用发送时初始化、由引擎上报更新的 `activeModel`；Claude 的 `haiku` / `sonnet` 等选择器别名不能压过已解析的自定义模型名。`sessions.ts` 的会话列表刷新不覆盖运行中已知的模型，标签页选择仍保留供下次发送。用量页按台账记录的模型名聚合，不按当前渠道映射猜测回填旧台账。回归：`usage-accounting.test.ts`、`session-model-memory.test.ts`。
- disabled 必须改变光标语义（`disabled:cursor-not-allowed` 或 `disabled:cursor-default`）并降低强调（`opacity-50`~`60` 或语义 disabled token），不能只是点不动。
- **异步动作进行中不可重入**：进行中禁用按钮（或首行拦截 `if (running) return`），避免重复请求。
- **反馈不改变布局**：图标在默认态与反馈态之间切换时，外层容器尺寸固定（`ActionFeedbackIcon` 用 `iconClassName` 同时约束容器和图标），按钮不能因为换图标而抖动。
- **动效可降级**：过渡类一律带 `motion-reduce:transition-none`；关键帧动画的降级见 [§8](#8-待收敛)。
- **同一状态只表达一次**：列表行里「已安装 / 可更新」只给一个信号——市场表的右侧按钮就是该行的状态（`安装` → `更新至 vX` → `已安装`），行内不再重复挂徽标；安装中按钮原地换成进度（`plugins.installingPct`）且保持占位不變（`PluginMarketRow.tsx`）。
- **表格化列表**：插件市场用语义 `<table>` + `table-fixed`，列头是唯一的字段说明（名称 / 开发者 / 安装量 / 版本 / 操作）；整列无数据时整列不渲染（`PluginMarketView` 的 `showDownloads`），不用一列「—」占位。开发者列的头像是该账号的真实 GitHub 头像（`githubAvatarUrl`），加载中或取不到时回落到同一配色的首字母瓷砖，不出现破图。
- **官方身份用紫色品牌徽标，工具栏下拉同时承担人群范围**：市场表开发者列在 `githubLoginFor` 解析出的账号等于官方账号时（`isOfficialPlugin`，账号 `zhukunpenglinyutong`，author 或 repo owner，大小写不敏感），整格只渲染紫色「CCGUI官方插件」徽标（`status-purple-background` / `status-purple-text`；紫色专属官方，不与中性类型徽标、lime「已安装」混用）——官方插件的账号是隐含信息，不再重复头像与（被截断的）用户名；详情页右栏 `AuthorChip` 同一条判定，徽标整块是按钮（`title` 报出目标主页），点击打开该官方账号主页，第三方插件才展示可点的头像+名称。工具栏下拉（`sortLabel`）语义混合：`综合排序` / `下载量` 显示全部、只是排序不同；`CCGUI官方插件` / `社区插件` 只保留该类并按下载量排序（`sortPlugins` 内 `pluginMatchesAudience`）。空状态的「清除筛选」要把下拉一并复位回 `综合排序`。
- **「已安装」的筛选按记录字段判定，时间只说安装时间**：页头「已安装」标题右侧的下拉（`installed-filter.ts` 的 `filterInstalledPlugins`）默认「全部」，其余三项是「最近安装」（`installedAt` 落在 3 天窗口内，恰好满 3 天仍算，按安装时间倒序——四项里只有它重排，其余保持后端 id 序）、「市场安装」（记录 `source === "marketplace"`）、「本地安装」（`source === "local"`）。`installedAt` 是 backend 的 Unix 秒（`state.rs` 的 `now_secs`），不是毫秒；重装 / 更新保留首次安装时间（`fs.rs`），所以一次更新不会把旧插件顶进「最近安装」，时间戳为 0 的记录不算。搜索框与下拉同时生效，筛到空且确有筛选条件时给「清除筛选」，把两者一起复位。回归：`installed-filter.test.ts`、`PluginHub.test.tsx`。
- **开发者只在能落到真实账号时可点**：插件详情页右栏的「开发者」用 `githubLoginFor({ author, repo })` 判定身份——索引 `author` 是 GitHub 账号（或回落到 repo owner）时，整块头像+名称是可点按钮，点击走 `openExternal` 打开 `https://github.com/<login>`，并把目标主页写进 `title`；官方徽标同理指向 `OFFICIAL_PLUGIN_LOGIN`；解析不出账号时保持纯文本，不猜主页地址（`PluginDetailPage.tsx` 的 `AuthorChip`）。
- **带背景的块在 flex 列里必须自适应宽度**：右信息栏 `RailRow` 是 `flex flex-col`，默认 `align-items: stretch` 会把任何块拉伸到整栏宽——带背景的徽标不加 `w-fit` 就变成整行色块。所以 `OFFICIAL_BADGE` 带 `w-fit`，可点的头像+名称块用 `flex w-fit max-w-full`。长文本靠内层 `truncate` 收窄，不靠父级的拉伸。
- **时间只说数据源里有的**：插件详情页右栏的「最近更新时间」只取索引 `plugins/<id>.json` 的 `updatedAt`（上游 Release 发布时间，`indexUpdatedAt` 解析后按当前语言格式化）；条目没有该字段就不渲染这一行，不用本机安装时间顶替，也不用「—」占位。
- **插件素材可选、缺失不占位**：插件图标取索引 `icon`（市场行、详情页头部、已安装行共用 `PluginAvatar`），加载中或取不到时回落同一 id 的确定性渐变首字母瓷砖；详情页效果图取索引 `screenshots`，空数组整个图集不渲染（`PluginScreenshotCarousel`），单张加载失败只在该槽位显示占位文案。不出现破图，也不用「—」占位。
- **大图预览必须有三条出路**：截图放大层（`PluginScreenshotCarousel` 的 lightbox，走 `ModalShell`）同时支持点空白背景、按 Escape、点右上角 `X`（`fixed right-5 top-5` 的 36px 圆形浮标，`aria-label` / `title` 为「关闭大图」）关闭。背景点击依赖 `ModalShell` 把 `isDismissable` 写在 `ModalOverlay` 上：react-aria 的 `useOverlay` 默认 `isDismissable = false`，`useModalOverlay` 只读 ModalOverlay 的同名属性，写在里层 `Modal` 上会被忽略（开发态有警告），表现为「点空白关不掉」。`X` 锚在视口角而不是图片角：效果图宽高比不定，锚图片要么盖住角落内容，要么随图片漂移。回归：`PluginScreenshotCarousel.test.tsx`。
- **插件面板页签只给图标**：聊天右侧面板页签条（`ChatPanelHeader.tsx`）里，插件页签（registry id 前缀 `plugin:`）只在 `PillTab` 的图标槽渲染 16px 图标，插件自报的 `label` 只作 `title` 与 `aria-label`（指针悬停 / 读屏可见，页签条里不占文字宽）；内建「文件 / 变更」保留图标+文字。插件没注册 `icon` 时回落同一插件素材（manifest 图标经 `plugin_read_artwork` → 市场安装会把索引品牌图按该相对路径落到插件目录，离线可用）→ 确定性渐变首字母瓷砖，与插件市场同一条链（`PluginPanelTabIcon.tsx`）。
- **设置页插件条目用插件自己的品牌图**：设置页导航（`SettingsPage.tsx`）里每个插件 section（registry id 前缀 `plugin:`）的 16px 图标，插件注册了 `icon` 就用它；没注册时回落该插件 manifest 的 `icon`（经 `plugin_read_artwork`，与面板页签、插件市场同一条素材链），只有插件没有品牌素材时才用共用的 `layout-grid` 兜底。这里不画市场同款渐变首字母瓷砖：导航栏其他行的图标都是单色 lucide，彩色瓷砖会喧宾夺主，没有真实素材时中性宫格才是这一列的基调（`PluginSettingsNavIcon.tsx`）。回归：`SettingsPage.test.tsx`。
- **页头文字按钮的两种禁用分开**：插件中心页头（`PluginHub.tsx`）同一种文字按钮分两个禁用语义——「进行中」用 `disabled:cursor-wait`（`HEADER_BUTTON_BUSY`），「前提不满足」用 `disabled:cursor-not-allowed` + `opacity-50`（`HEADER_BUTTON_BLOCKED`），且后者必须给 `title` 说明缺什么（如「创建插件」在没有工作区时不可点）。等待态不能用来表达「你还没准备好前提」。
- **聊天输入框回车与中文选字**：`composer-editable.tsx` 通过同步组合输入状态、`KeyboardEvent.isComposing` 和 IME `keyCode === 229` 区分候选确认与发送。组合输入期间按键交给输入法；组合结束后迟到的 IME 确认回车也只跳过应用快捷键处理，不发送、不取消输入法默认行为。选字后独立回车立即按发送快捷键设置处理，不设时间保护窗；Shift+Enter 换行，Cmd/Ctrl+Enter 模式下普通 Enter 换行。候选确认键也不触发补全菜单或历史召回；`usePromptHistoryNav.resetNavigation` 在开始组合输入时退出历史导航，保留编辑后的正文，避免选字后方向键恢复旧草稿。回归：`composer-editable.test.tsx`、`ai-chat-composer.test.tsx`。
- **粘贴文件与拖拽走同一条路由**：从文件管理器复制的文件（Finder / Explorer）在输入框粘贴时与拖入完全同构——图片进附件管线，其他文件在光标处变成 `@path` 引用，不把不透明的 `File` blob 当成「没有内容」静默丢弃。`composer-editable.tsx` 用 `readClipboardFiles` 分类事件里的图片字节 / 其他文件 / `text/uri-list`：非图片文件先向宿主探测系统剪贴板（`clipboard_file_paths`：macOS 读 NSPasteboard 的 file URL、Windows 读 CF_HDROP），探测为空时用事件里的 `text/uri-list`（GTK 一类剪贴板会直接暴露路径）兜底。截图（⌘⇧4 之类）仍走原有字节管线保存为附件，不受影响；纯文本粘贴行为不变（只取 `text/plain`，不泄漏 HTML）。没有活动会话时（`onPastePaths` 缺席）粘贴文件不落地，与拖拽一致。回归：`composer-editable.test.tsx`、`clipboard-files.test.ts`、Rust `clipboard::tests`。
- **跳转后必须真的给光标**：从插件中心/浏览器/文件切回聊天（「创建插件」「新建会话」）时，输入框要真的获得焦点——中心面用 `.invisible` 切换，隐藏元素上的 `focus()` 会被浏览器静默忽略（fixture 实测：切换到可聚焦要 ~250ms）；统一走 `src/features/chat/focus-composer.ts`，它在时间窗内逐帧重试，并在焦点落到可见输入框时立即停手。**预填草稿的光标由我们自己落位**：草稿恢复会重建 editable 的 DOM，浏览器手里的插入点随之消失，随后 `focus()` 会把光标搁回内容开头；`Composer` 的外部 value 同步（`replaceEditableText`）在重建后把插入点放到文本末尾，用户可直接接着敲需求。回归：`tests/browser/creator-jump.html`（断言输入框内容是预填原文、光标在文本末尾）、`ai-chat-composer.test.tsx`。
- **对话区分屏（Trellis 式）**：中心区在分屏开启时按递归树渲染分屏格（`src/features/chat/split/tree.ts` 的 `layoutGeometry` 把树摊平成绝对定位矩形，增删格子只改坐标、不换组件层级，已挂载的对话不重挂）；`root === null` 就是原来的单栏（无格子标题栏），只剩一格自动回单栏。入口：从侧栏会话行、或从页签条里的会话页签（向下拖出页签条约 24px 即交接给分屏层，横向拖动仍是排序/`use-tab-drag-reorder`）拖到中心区（**边带 = 在该方向切分**，**中心 = 放入/替换**；拖动的是已分屏会话时中心是移入、边带是整格搬家；拖动格子标题栏到另一格是整格移交、中心是内容互换）、格子标题栏的「向右 / 向下分屏 / 关闭这一格」、侧栏会话右键「向右 / 向下分屏」（等价于把该会话放到聚焦格的对应边）、页签条右键「退出分屏」。**聚焦格 = 页签条高亮的那条对话所在的格子**：点格子任意处即聚焦并激活其会话，点侧栏会话若已在分屏里就聚焦那一格、否则替换聚焦格的内容（空格子是被替换的目标，不抢焦点）。每个格子有自己的输入框、队列与运行状态（`send` / `queueMessage` / `interrupt` / `loadEarlier` / `moveQueued` 等按格子会话定向，授权 / 问答 / 计划审批卡从 `SessionScope` 取本格会话 key，`composerBridgeRef` 把「聚焦输入框 / 插入 @path」解析到聚焦格）；`/mcp` 面板是全局单例，只在分屏层挂一次。拖动分隔条改比例，两侧各保留最小 220px（纵向 140px），超过就夹住；拖动与拖拽落点提示期间关掉位置过渡。布局持久化在 `localStorage`（`ccgui-next.splitLayout:v1`），恢复时校验结构与会话；页签被关掉的会话，它所在的格子一起消失（不额外关会话），回到单栏时清掉持久化。回归：`split/tree.test.ts`、`split/store.test.ts`、`split/SplitLayout.test.tsx`、`use-tab-drag-reorder.test.tsx`（页签横向排序 / 向下交接 / 交接被拒回落）、`tests/browser/split-layout.html`。
- **激活哪一个面，哪一个面就必须真的在视**：中心区同一时刻只有一个面在视（对话 / 文件 / 浏览器 / 插件页签 / 插件中心 / 任务工作台 / 版本更新说明 / 差异），互斥靠各激活入口维护（`ChatCenterPane.centerSurfaces` 只按布尔量判定可见性）。新建会话（侧栏、页签条 `+`、快捷键）、工作区行 `+`、点击会话线程、新建浏览器、打开文件（文件树/搜索/插件桥）以及插件的 `openCenterTab` / `selectSession`，在激活自己的面之前必须清掉其他面（`src/features/chat/center-surfaces.ts` 的 `dismissCenterSurfaces`；文件侧是 `files/store.ts` 的等价清场，先清后设 `activeFilePath`），否则页签条已经高亮到新页签、画面还停在上一个面。回归：`use-chat-sidebar.test.tsx`、`files/store.test.ts`。
- **⌘W 关的是标签页，不是窗口**：`closeTab` 快捷键动作（默认 ⌘W，设置 → 快捷键可改）注册在 `use-chat-page-effects.ts`，走 `useChatTabs` 的 `handleTabClose`——与点页签 × 完全同一条路径：脏文件仍弹保存确认（未保存内容从不静默丢弃），其余页签（会话 / 文件 / 浏览器 / 插件 / 单实例）立即关闭，没有页签时 no-op。macOS 上 Tauri 默认菜单把 ⌘W 绑给原生 Close Window（`performClose:`，直接销毁窗口，前端收不到 keydown），所以 `src-tauri/src/app_menu.rs` 重建了同一套应用菜单、只拿掉该项；没有菜单项认领这个键，键事件才能到 webview，与 Windows / Linux 一致。About / Quit（⌘Q 仍受 `quit_guard`）/ Edit / Window 菜单保持不变。
- **更新说明页签与浮层提示各管一摊**：更新检查发现新版本时，除右下角浮层提示（`UpdateToast`）外还自动把发布说明开成中心页签（原生单实例，`useReleaseNotesTabStore`，模式同插件中心 / 任务工作台），排在页签条最尾；状态栏版本号按钮与命令面板「查看版本更新说明」（`builtin:openReleaseNotes`）打开的是同一个页签。浮层「稍后」只收起待更新状态与提示，页签里的说明继续可读（`notesRelease` 快照不随 `dismiss` 清空）。页签正文优先渲染更新清单的 `notes`（新版本自带的单语 markdown），没有时回落本地 `CHANGELOG_DATA` 里**同版本**条目（双语排序，markdown 映射就在 `ReleaseNotesPane.tsx` 内），两者都没有时明说「这个版本没有附带更新说明」——不把相邻版本的说明挂在当前版本标题下。页签是最弱的单实例面：自动弹出时不抢已在视的插件中心 / 任务工作台（`centerSurfaces` 的优先级）。
- **更新页签的页头就是更新入口**：发现新版本时给「立即更新」，任何时候都能就地「检查更新」。检查是刷新型动作，走 §4.1 的转圈 → 对号，**检查失败（store 的 error 阶段）不出对号**（`useActionFeedback` 的 `isFailure`）；检查中与下载/安装期间按钮禁用，不重入。结果行与设置页（`UpdateSection`）共用一份文案（`useUpdateDescription`）：检查中是「正在检查更新…」，已是最新给「当前已是最新版本 · 最新版为 vX（日期 发布）」——日期用清单的 `pub_date`，按当前语言格式化，取不到日期就只说版本；失败行用 `role="alert"`，重试就是同一个「检查更新」按钮，不另开第二个按钮。检查更新不是列表重新读取，故不进 [§7](#7-刷新入口清单)。按版本翻页的「版本记录」弹窗与本地历史翻页已随页签上线下线（页签只展示本次发现 / 最新一条）。回归：`ReleaseNotesPane.test.tsx`、`UpdateSection.test.tsx`、`app-status-bar.test.tsx`。
- **升级后首启自动宣布新版本**：应用版本比上次运行真的前进了、且本地 `CHANGELOG_DATA` 里有这个版本的条目时，首启自动把版本更新说明开成中心页签并标记未读（`src/features/update/upgrade-announcement.ts`；上次运行的版本记在 `localStorage` 的 `ccgui-next.lastSeenAppVersion:v1`）。**只在首次升级后展示**：首次安装只写基线不弹（没有可对比的旧版本），版本没变、降级、版本号认不出（预发布后缀之类）、本地没有该版本条目都不弹，且基线照样前进，不会每次启动重来。未读标记是两处：页签条上的强调色圆点（`SessionTab` 的 `unread`，自带可访问名「新版本」）+ 页头版本号旁的「新版本」胶囊；用户关掉页签即视为已读，两处标记一起消失。版本号与正文必须同源——未读标记指向的版本号与本地条目的版本号是同一个（`ReleaseNotesPane` 里 `unreadVersion` 参与版本优先级），不拿最新一条顶替。更新检查发现的待更新版本不写未读标记：那个场景已经有浮层提示与「立即更新」。回归：`upgrade-announcement.test.ts`、`ReleaseNotesPane.test.tsx`、`use-chat-tabs.test.tsx`、`session-tab-strip.test.tsx`。
- **详情页的滚动契约**：`lg` 上右信息栏 sticky 之外还要有高度上限和自己的滚动（`PluginDetailPage.tsx` 的 `RAIL`：`lg:max-h-[calc(100dvh-10.5rem)]` + `lg:overflow-y-auto`）——权限展开后信息栏可以比窗口高，只 sticky 不限高会把它压在视口里，「链接」等末尾行要把左侧 README 滚到底才看得到。左栏 README 的代码块由 `prose-plugin-readme pre`（`src/index.css`）自己横向滚动：单行超长命令在正文列内滚动，不允许画到右信息栏上。
- **插件权限必须自称归属**：插件详情页右栏的权限行标签是「权限（CCGUI权限）」（`plugins.hub.permissionsTitle`）——只写「权限」会被读成电脑系统权限，括号里的归属是必需的，不是可选修饰；中英文同步（`Permissions (CCGUI)`）。该行的 `permissionsEmpty` / `permissionsCount` 与列表项语义不变。不要与聊天输入框的引擎权限模式（`plugins.hub` 之外的 `permissions` / `permissionLabel`）混用同一处修改。
- **「链接」三项各带目标图标**：插件详情页右栏的仓库 / 发布记录 / 问题反馈在文字前各给一个 14px（`size-3.5 shrink-0`）lucide 图标——GitHub 标记（`github`）、发布标签（`tag`）、issue 圆点（`circle-dot`），三个目的地不读文字也能分开；图标 `aria-hidden`，可访问名仍只有链接文字。文字后的 `square-arrow-out-up-right` 保留：图标说明去哪儿，箭头说明会离开应用，两者不互相替代（`ExternalLink`）。回归：`PluginHub.test.tsx` 详情页用例。
- **浮动滚动浮标方向跟随滚轮**：聊天时间线的浮动控件（`ScrollControl.tsx`）只在用户滚轮后出现——向上滚显示「回到顶部」（`ArrowUp` / `chat.backToTop`，点击暂停跟随后平滑滚回顶部），向下滚显示「回到底部」（`ArrowDown` / `chat.backToBottom`，点击恢复跟随并平滑滑向尾部，落定后再硬钉一次吸收动画期间长高的内容）；仅在内容不足一屏、已在底部（距底 100px 内）或滚轮停下 1.5s 后隐藏。`scroll` / `resize` 只负责隐藏、从不主动显示，所以流式钉底不会闪出浮标；平滑滑向尾部的整个过程中自动钉底让位（`use-scroll-follow.ts` 的 `smoothPinRef`），避免中途一次流式刷新把过渡掐断；`prefers-reduced-motion` 下两侧都改为瞬时跳转。
- **智能体和提示词是两个独立设置页**：设置 → 系统 rail 下「智能体」与「提示词」各占一行（`sections.tsx` 注册 `agents` / `prompts` 两个页面，组件直接是 `BotsPane` / `PromptsPane`，不再有「智能体与提示词」页签壳）；输入框 `#` 菜单与 `!` 菜单的「新建」行分别深链到 `?page=agents` 与 `?page=prompts`。不等同于把两类数据混成一页再分页签：提示词页只展示自定义提示词，智能体页只展示 Bot。设置搜索里智能体页索引自定义 / 内置目录两个页签，提示词页的列表是用户数据、只靠页名进入。回归：`SettingsPage.test.tsx`、`builtin-search.test.tsx`。
- **智能体 = 身份常驻 + 分区切换，配置必须能解释成提示词**：设置页 → 智能体的列表里，列表行是「名称 + 头衔 + 简介 + 运行后端 + 技能数」，点开进入**弹窗**编辑器（`bot-editor.tsx`，走 `ModalShell`：`min(1180px,94vw) × min(780px,90vh)`，Esc / 点背景 / 右上角 X 三条出路，列表就在背后一步之遥）。编辑器左栏是身份（生成形象 / 表情符号 / 图片三种头像来源 + 名称 / 头衔 / 简介 / @标识），**固定 400px 宽**（标签列 96px 是定值，再窄就把输入框压到不好用），**独立滚动**：窗口不够高时这一栏自己滚（身份卡片 / 提示文案 / 底部信息各自的 `shrink-0` 是这条契约的实现——缺了它弹性收缩会把卡片压扁再被自身的 `overflow-hidden` 剪掉，整栏既看不全也滚不动；400px 与这组 `shrink-0` 由 `tests/browser/bot-editor.html` 实测），右栏按分区切换、同样独立滚动，**身份不随分区切换丢失**；弹窗内部三处滚动容器（身份栏 / 分区页签条 / 分区内容）都 `scrollbar-none` 不画滚动条——11px 的轨道贴在身份栏的分隔线上看像第二条竖线，设置页内容列与页签条本来就是这么做的，溢出提示交给被截断的内容本身。编辑是自动保存的：`draft` 本地渲染、600ms 防抖后 `bot_update`，头部状态位在「保存中 / 已自动保存 · 时间 / 保存失败：原因」之间切换，后端归一化的字段（slug 唯一性、无字形的 emoji 头像）以返回值为准，不给用户回写他没敲的内容。未实现的分区**显示但没做完就说没做完**（页签后缀与分区标题都是「即将支持」，正文是一张**只读的概念流程图**，`bot-concept-diagram.tsx`），不隐藏——用户看到过发布说明就得能找到入口；记忆分区已上线，页签常驻、正文是上面那条真面板说明，其余三个仍按概念图展示。概念图是抽象流程（方框 + 箭头），节点用与任务画布节点同一套外观（`rounded-lg` 边框卡片、图标列、`text-body-2-medium` 标题 + `caption-1` 说明、状态色 token），只有「用真组件画出来的样子」而没有真控制：**图里没有 input / switch / button**，也不带 hover 与点击态（`BotsPane.test.tsx` 断言图内零交互元素）——功能没上线就画一个能按的开关，比一句「即将支持」更容易被当成 bug。行走的是内部路线图（`计划.md`、`bot-prompt.ts` 的 `planned` 标记、后端 `BotToolConfig.phase`），**不进界面**：「阶段 3」会被读成用户可以等的版本号。头像工作室的形状 / 颜色 / 表情三组控件都用**当前其它选择**做预览（换颜色时形状行跟着变色），候选按钮带 `aria-pressed` 与 `aria-label`。生成形象就是 BoardUI Pro 的 agent-creator 头像引擎（`src/components/application/agent-avatar/*`，canvas 绘制：九种折纸轮廓、十六种表情、HSL 颜色，带折面明暗、颗粒、眨眼、视线游走与换形动画）；**头像不再提供「表情符号 / 图片」两种来源**——已存在的 emoji / 图片头像照常渲染，编辑器只给一行说明，用户动形状/表情/颜色时才替换（不静默改用户原来的图标），舞台右上角的骰子一键随机（形状 + 表情 + 预设色，颜色只从设计好的九个预设里取，不会随机出浑浊色）；我们的 `features/bots/bot-avatar.tsx` 是**唯一**的存储 → 引擎适配点（`normalizeAvatar` / `avatarConfig`），列表、编辑器、`#` 菜单、输入框徽标、**用户气泡上方的智能体徽标**都走 `BotAvatarView`，不允许各自画一套——徽标手里只有提交进正文的 `Bot Id` 与 `Agent Icon`，所以先查当前 Bot（同一 Bot 改过头像，旧消息也显示新形象），查不到（已删除 / 内置目录）才用记录下来的图标兜底：emoji 原样、其余按 id 确定性生成，槽位不留空；**每个尺寸都跑同一套动画**——36px 的列表行、16px 的 `#` 菜单行、14px 的输入框 / 聊天徽标与编辑器大预览一样眨眼、游走视线，`paused` 只留给明确要静帧的调用（形状弧的轮廓预览）；所有实例共用 `frame-loop.ts` 的一个 `requestAnimationFrame`（最后一个订阅者退出即停，画布拿不到 2D context 时根本不订阅），不是每个头像各起一个循环。老数据的 emoji 图标原样保留；旧应用里那批 ASCII 预设 id（`agent-robot-06`）在迁移时变成**按 id 确定性生成**的形象——同一份数据每次渲染长得一样，绝不把预设 id 当文字塞进提示词或界面。回归：`BotsPane.test.tsx`、`bot-prompt.test.ts`、`selected-bot.test.ts`、`frame-loop.test.ts`、`MessageTimeline.test.tsx`、`tests/browser/bot-editor.html`。
- **记忆是真面板：两个账本 + 三个开关**：页签常驻（`memory-section.tsx`），不再走概念图（运行后端 / 定时任务 / 协作仍是「即将支持」概念图，`PLANNED_TABS` 已不含 memory）。两个账本按「这个智能体 / 全局」明确分区：MEMORY 属于当前 Bot，USER 全局共用、改它会影响所有智能体——标题与说明就写这件事，不做成两个看不出区别的列表。用量条读的是**注入时的字符数**（每条渲染成 `- 内容` 再连接，与后端 `memory::used_chars` 同一规则），不是裸内容长度，所以界面上的数字就是模型读到的额度；>80% 变橙、超限变红。写入规则只有后端一处：面板的手动增删改和模型调用的 `memory` 工具走同一套闸（安全扫描：提示注入 / 密钥凭证 / 不可见字符；容量：超限**拒绝**并回传当前条目与用量，由调用方合并后重试，绝不截断或静默丢弃），错误按 `code` 本地化，扫描细节（命中哪一类、具体片段）原样附在括号里——只给类目用户不知道怎么改。删除一条与清空一个账本都走 `ConfirmDialog`（danger，用 §6 的破坏性操作规范）：单条问「删除这条记忆？」，清空在文案里点名是哪个账本；导出 MEMORY.md / USER.md 走 `pickSavePath` + `ipc.writeFile`（web 端降级为下载）。三个开关都已接上，记忆关闭时审批 / 复盘开关禁用（没有可作用的对象）。**写入需要审批**：开启后模型的 `memory` 工具写入（以及后台复盘写入）不进账本，先落成待审批条目（同一写入重试不堆重复；空内容 / 安全扫描 / `old_text` 匹配不到在暂存时就退回，模型当轮能改）；面板在开关下方列出待审批队列，每条标「新增 / 修改 / 删除 + MEMORY / USER + 模型写入 / 后台复盘」，replace 与 remove 用删除线展示暂存时锁定的原文、新增内容正常展示，逐条或全部批准 / 驳回；批准时才过容量闸，超限保留条目就地报错；replace/remove 暂存时记下目标条目 id 与原文，批准时原文已变（用户手动编辑过）拒绝执行并提示驳回重来（`stale`），绝不覆盖用户的修改；面板手动写入不经过审批。**会话结束后台复盘**：每累计 `reviewEveryNTurns`（默认 5）轮、以及离开会话（关闭标签 / 切换到另一个会话）时还有未复盘的轮次，就在后台用该引擎已配置的 API 渠道发一次 HTTP（模型默认取当前会话的模型），把值得保留的偏好 / 纠正 / 环境事实整理成严格 JSON 后逐条走同一套写入闸；没有配置渠道、官方登录、同一 Bot 已有复盘在跑时明确跳过，开关下方就地显示「上次复盘：写入 n 条 / n 条待审批 / 无内容 / 跳过原因 / 失败原因」，不静默失败；复盘写入的条目来源标「后台复盘」。工具可用性按引擎（`EngineInfo.supportsMemory`，目前 Claude Code / Codex / omp）：引擎不能挂载时提示词里省掉「记忆使用说明」——不能教模型调用不存在的工具；已有记忆仍照常注入，页签里写明哪些引擎真能写入。关闭标签与切走之外（应用退出、归档、删除会话）不补复盘，复盘是尽力而为的后台动作，不阻塞任何界面操作。回归：`BotsPane.test.tsx`、`bot-block.test.ts`、`memory.test.ts`、`memory-review.test.ts`、Rust `memory::tests` / `memory::pending::tests` / `memory::review::tests` / `memory::mcp::tests`。
- **「说的」和「做的」分两页，拼装结果必须能看见**：人格（SOUL）与工作规则（AGENTS）各自一个 Markdown 字段、各自有说明与占位示例，但合计受同一个 10,000 字预算约束（底部一条用量条，>80% 变橙、超限变红）。两页都做**重复行检测**（去掉列表符号后 ≥6 个码点才算，避免「说人话」这类短句误报），命中时给一条黄色提示并展示那行原文——不给块状错误，也不自动删改用户文字。编辑器右上「拼装预览」抽屉（`bot-prompt-preview.tsx`）按固定顺序列出每个区块的**字符数与开头一行**，空区块标「已省略 · 空」，未实现的区块标「即将支持」，引擎自己提供的基础提示标「引擎提供」——预览用的是编辑器里的**实时草稿**，而发送用的是会话开始时冻结的快照（`selected-bot.ts` 的 `block`），两者语义不同，所以抽屉头部写明「会话开始时冻结」。`#` 选择器的每一行现在带头像 + 头衔/简介 + 运行后端徽标与技能数，置顶分组成「置顶」；被隐藏的 Bot 只在设置页出现，不进 `#` 菜单。
- **可折叠分组标题的箭头尾随标签**：设置页导航（`src/components/application/settings/settings-shell.tsx`）里可折叠分组的标题行是「标签 + 右侧箭头」——箭头只占行尾，标题文字留在与静态分组标题（如「插件」）相同的左侧内边距列上，而不是被头部箭头推进条目图标列；展开只转箭头（`rotate-90`），`aria-expanded` 同步。
- **行内只画「有副本」的引擎**：能力扩展 → Skills 行的引擎同步态只渲染真有副本的引擎图标（`TargetEngines`，`src/features/skills/components.tsx`）：彩色=已同步、右下角红点=副本丢失（orphan，点它即重新同步）；点已同步图标会移除该引擎副本（未纳管技能自己目录里的副本除外，它禁用取消并在 `title` 说明「你自己的本地副本；应用不会删除它」；这个禁用**只改光标与 `title`，不降图标透明度**——半透明图标读起来像副本丢了或渲染坏了，而这里要说的恰恰是「有副本、只是不能在这里删」，透明度只留给进行中的动作）。没有副本的引擎**不占行内位置**——一份技能在 13 个引擎里只剩 1~2 个图标，而不是铺一地淡图标；未安装且无副本的引擎不渲染（后端 `available:false`），但已有副本或副本丢失的引擎必须保留，否则清理路径就消失了。「加一个引擎」在详情面板的「同步到」里做，未纳管技能的「纳管」也只在那里（行内不挂这个按钮：本地技能多的时候列表右侧会排满按钮，而且纳管是低频的一次性动作，不配和同步态图标抢位）。
- **发现页先让人看懂再让人装**：skills.sh 的搜索/热门只返回 name / repo / installs（描述字段后端固定为空串），行主体点开详情（`SkillDiscoverDialog.tsx`）按需调 `remote_skill_content` 回仓库读 `SKILL.md`——列表不预取，几十行 × 两个 GitHub 请求会直接撞限流。详情先给 frontmatter 描述与正文，再看安装按钮；读不到时给「仓库里没有这个技能的 SKILL.md」的说明 + 仓库入口 + 重试，不把后端英文原文当用户文案。id 与仓库目录名不一致时按「同名 / 去掉仓库前缀 / `:` → `-`」对齐（`vercel-react-best-practices` → `skills/react-best-practices`），对不上报 `not_found`，不拿别的技能正文冒充；安装走同一套对齐规则（`resolve_existing_skill_dir`），否则这些条目会死在「SKILL.md not found」。没有描述时行里第二行就是 `owner/repo · 安装数`，不再把仓库重复贴两遍。图标是行按钮的兄弟节点，点它不会顺便打开详情面板；每个按钮带 `aria-pressed`（synced 为 true）与 `title` / `aria-label`（「{{引擎}} 已同步 / 副本丢失 / 无副本」），不把颜色当唯一信息。
- **配置态与运行时态分开表达**：能力扩展 → MCP 页把「配置已启用」（CLI 配置文件里的状态）与「运行时已连接」（某次会话实际加载的服务）拆成两个清单：运行时条目必须带来源会话与采集时间，没有会话 / 引擎不支持查询时用状态文案说明原因，不显示成「没有服务」。配置条目里，不可安全写入的来源只渲染带 `title` 原因的锁图标（`src/features/mcp/McpSection.tsx`），不渲染不可用的开关；开关只对已验证写入语义的来源开放。
- **MCP 页要显式表达「这个 CLI 支不支持」**：引擎选择行列出后端 `ENGINES` 里的全部引擎（含未安装、含不内置 MCP 的），引擎级状态由 `support` 字段给出——`native` 正常展示清单，`plugin` 说明 MCP 由插件提供（dsh 列出 profile 里的 `dsh-mcp-client` 实例），`none` 只给「未内置 MCP」说明、不渲染空清单（`pi` 属于此类，不为它伪造来源）。只读原因用可本地化的原因码（`mcp.readonlyReason.*`，缺失时回落后端字面文案）。清单为空时列出本页读取的来源文件（`sources`，含尚未创建的并标注「尚未创建」），把「没配」与「不支持」分开。引擎支持深链 `#/settings?page=mcp&engine=<id>`（`engineIdFromHash` 从 hash 读取，区块在 Router 外也能渲染）。回归：`McpSection.test.tsx`。
- **引擎选择器与 Skills 同形**：MCP 页的引擎行用与 Skills 相同的 `Chip`（`src/components/base/chips/chip.tsx`，两处共用）+ `EngineIcon`（12px）+ 品牌名，有配置时在名字后跟条数；不用蓝色 PillTab 条——两个「能力扩展」页面的引擎筛选应该长得一样。范围筛选（全部 / 配置 / 运行时）也用同一颗 Chip。
- **`/mcp` 面板与设置页同源**：输入框的 `/mcp` 既是 `/` 选择器里的内置行（`app-commands`；用户自定义同名目录命令优先），也是直接提交的命令；点该行或提交都打开 `McpCommandPanel`，按当前会话引擎列出配置清单与运行时状态。数据与设置页共用同一个 `mcp_inventory` 调用（`useMcpInventory`），两处不存在第二份口径；面板提供刷新（§7 登记）、可写来源的开关、连接检测、条目详情弹窗与「在设置中管理」深链到对应引擎页签（`openMcpSettings`）。引擎不支持 MCP 时面板同样只给说明。回归：`McpCommandPanel.test.tsx`。
- **连接状态由本应用显式检测，打开页面即跑，并带缓存与并行上限**：面板与设置页出现时自动开跑（`useAutoProbe`），但只针对「启用 + 可检测 + 没有新鲜结果」的条目，指纹由「启用条目的 id + 配置哈希」组成，所以重复打开只吃缓存、配置一变只补变了的那几条。检测会按配置真的启动 stdio 服务（`npx` 可能触发下载）或连远程地址，做 `initialize` + `tools/list` 握手后立即结束进程（整组杀，不留孤儿子进程），状态落在行内徽标：已连接（绿色，带工具数）/ 需要登录 / 连接失败（原因在 `title` 与详情里），结果带检测时间（标题行「状态更新于 …」）、耗时、服务名与工具名（`probe-ui.tsx`）。新鲜度窗口 `PROBE_TTL_MS` = 3 分钟（`probe-store.ts`），窗口内自动检测直接复用，工具栏「检测全部」是强制重跑；同时最多 `PROBE_CONCURRENCY` = 4 个在飞（stdio 启动是 I/O 等待，串行太慢，全并行会同时拉起一堆 npx）。本机回环地址不走环境代理（`HTTP_PROXY` 会把 127.0.0.1 请求变成 502）；`${VAR}` / `${VAR:-default}` 按 CLI 习惯展开；返回缺字段的响应不当作已连接（边界校验）。这与运行时分区（CLI 会话自报的连接状态）是两个概念，页面上分开表达。回归：`probe-store.test.ts`、`McpSection.test.tsx`、`McpCommandPanel.test.tsx`。
- **Claude 的用户 / local 来源可以就地启停**：启用/停用写 `~/.claude.json` 的 `projects[<工作区>].disabledMcpServers`（与 Claude Code TUI 的「停用（本项目）」同一把开关，已用 `claude mcp list` 对拍：列表显示 ⊘ Disabled），不往服务定义里塞 `enabled`；没有活动工作区时该来源降级为只读（`mcp.readonlyReason.needs_workspace`）。工作区键优先按原样匹配，再回退 `canonicalize`（Claude Code 用 realpath 作键）。
- **禁用目标不能谎报**：Skills 详情里的目标复选框对只读来源（内置 / 系统 / 插件）禁用并同时给出只读原因文案（`skills.readonly.*`），不用静默过滤把只读来源「藏掉」。移除操作要分开「已删除」与「保留了你自己目录里的副本」（后端 `kept`）：后者不能报成「已移除」，否则刷新后图标还在，自相矛盾。
- **多引擎列表自带滚动，底部动作必须留在框内**：Skills 详情（`SkillDetailDialog.tsx`）的「同步到」最多 13 个引擎，整页内容（描述 / 属性 / 活动情况 / 同步到 / SKILL.md）放在同一个滚动体里，同步列表自己再限高滚动（`max-h-[13rem]`），「从所有 Agent 移除 / 更新 / 关闭」固定在框底——引擎变多不能把底部动作推出可视区。
- **终端路径链接用修饰键点击才唤起文件管理器**：终端输出里的绝对路径（`src/features/terminal/links.ts`）悬停仍有下划线与手型，但普通单击不再直接打开——只有 macOS `⌥`+点击、Windows/Linux `Ctrl`+点击才 reveal（`holdsRevealModifier` 从 xterm 传来的 `MouseEvent` 取修饰键；非 mac 选 Ctrl 与 Windows Terminal / GNOME Terminal 的开链习惯一致）。理由是选中文本、点回窗口很容易碰到链接，无修饰直接唤起访达的干扰太大。macOS 同时把 xterm 的 `altClickMovesCursor` 关掉（`TerminalView.tsx`）：同一个 `⌥`+点击否则还会把 shell 光标挪到点击处；Windows/Linux 保留该功能（那里的 reveal 手势是 Ctrl）。右键菜单里的「在访达中显示」不受影响——显式动作不需要修饰键。回归：`links.test.ts` 的修饰键用例。
- **分支选择器列远程分支并标「远程分支」**：变更面板与状态栏的分支列表（同一份 `git_branches` 数据）在本地分支之后列出 remote-tracking 分支（`origin/x`），行尾挂中性徽标「远程分支」（`git.remoteBranch`）——刚 fetch 到、本地尚无同名分支的远程分支必须可搜可切，与 VSCode / CLI 一致。选择远程分支不直接进入 detached HEAD：后端物化为同名本地跟踪分支（已存在则切到它，绝不以远程 tip 覆盖本地提交）；`origin/HEAD` 这类符号引用不进列表。列表仍按「本地在前、远程在后」分组，搜索仍是子串匹配。回归：`git.rs` 的 `branches_list_*` / `checkout_remote_branch_*` 用例、`ChangesPanelHeader.test.tsx`。
- **Worktree = 侧栏子工作区**：workspaces 表以 `kind="worktree"` + `parentId` 表达子工作区（`worktreeMetaOf()` 从 `meta.worktree` 读分支/PR 元数据），侧栏把它挂到父仓库行的「WORKTREES · n」分组内（`repo-tree.tsx` 的 `WorktreeGroup`）：子行主名是分支名（目录名进 tooltip），可展开各自的会话线程（展开态复用侧栏持久化展开集），分组整体也可折叠（折叠集存在 worktree store 的 localStorage）。分组与子行两层的展开/收起都走 [§2.4](#24-展开--收起动效) 的 `SidebarDisclosure` 高度动画。行内徽标：「PR#n」（仅从 PR 创建时，`status-purple-*`）。子行折叠且名下有会话流式中时，行内聚合显示同一套 `sidebar-thread-status` 呼吸点（全部流式会话都在退避重试则降为静态点），展开后让位给各线程行自己的状态点，两层不同时出现。父行不可见（已归档/已移除）时子行降级为普通顶层行，不丢入口；子行悬停出现 ＋（`chat.newSession`），直接在该 worktree 目录下开新会话（复用工作区行的 `onNewSessionInWorkspace` 链路）。分组只在有子项或有进行中创建时渲染，首个创建入口在工作区右键菜单「新建 Worktree…」；创建对话框三来源 Chip 顺序为「新分支（默认）/ 已有分支 / 从 PR 创建」。「新分支」的默认 base 是父工作区当前检出分支（取自 git store 的实时 status，不用会过期的列表标记，`defaultBaseRef()` 是唯一规则来源）——分支从手头这份工作接着往下开，与裸 `git worktree add -b` 取 HEAD 一致；当前分支是 main/master 时改用远程同名分支（本地 main 可能落后，远程 base 后端会先 fetch），detached HEAD 或该分支已不存在时落回「origin/main → origin/master → main → master → 任意远程 → 首个分支」的兜底顺序，用户手选后不再被晚到的 status 改写。回归：`WorktreeCreateDialog.test.tsx` 的默认 base 用例与 `pr-input.test.ts` 的 `defaultBaseRef` 用例、`use-chat-sidebar.test.tsx` 的挂载/降级用例、`ai-chat-sidebar.test.tsx` 的子行 ＋ 与分组折叠用例、`tests/browser/sidebar-collapse.html`。
- **Worktree 目录丢失与锁定要明说**：后端 `git_worktree_list` 解析 porcelain 的 `prunable` / `locked` 属性。`prunable`（目录已从磁盘消失）的子行渲染「目录已丢失」徽标（`status-rose-*`，原因进 `title`）；`locked` 的 worktree 在右键菜单里「删除 Worktree…」禁用并给出原因（对齐「禁用目标不能谎报」），删除对话框打开时同样复检。回归：`git_worktree.rs` 的 porcelain 用例。
- **⌘L 面板的检索统计行**：内容 lane 存活时（查询 ≥ 2 字且「内容」筛选开启），输入框下方常驻一行 `role="status"`：防抖与请求期间「正在检索…」，完成后「耗时 {{time}} · 共检索 {{total}} 条消息」，失败给「检索失败」（错误色）；三种状态共用一行，不从上一句查询留数字下来。`elapsedUs` 只计后端查询本身（FTS/LIKE 扫描 + 片段构建，`history/search.rs` 的 `search()`），`pending_count` 与语料 `COUNT(*)` 这两个看板查询在计时外；`total` 是当前 `session_messages` 条数（正在建索引时另按 `pending` 提示，不把二者相加）。时长文案由 `formatSearchDuration` 统一：< 10ms 保一位小数（`0.4 ms`），秒内取整毫秒，跨秒两位小数秒。回归：`session-search-palette.test.tsx`、`search.rs` 的 `total_messages` 断言。
- **设置页搜索要搜到页面里的行，命中带路径、跳转后高亮**：左导航搜索框（`src/components/application/settings/settings-shell.tsx`）分两条 lane：「命中行」lane 按目标页分组（组标题=页面名，行内两行是「行标签 + 所在卡片」，连起来读就是路径 `通用 › 外观 › 主题`），点选或回车打开该页、滚动到那一行并给行画 1.2s 内侧 focus ring（`settings-rows.tsx` 的 `SettingsRow anchor` + flash context）；「命中页面」lane 保留原来的页标题子串过滤，两条 lane 都空才显示「没有匹配的设置」。行索引是**声明式**的（`src/features/settings/settings-search.ts` 的注册表 + 内置页清单 `builtin-search.ts`，跟着页面在 `sections.tsx` 一起注册）——不预渲染页面扫 DOM：`GeneralSection` 挂载就读设置、`PetSection` 还会拉宠物列表，插件页是任意代码，隐藏挂载会带来真实副作用。行藏在页签或折叠卡片后面时，索引项带 `activatorAnchor`：shell 先把那个页签/标题点开（`aria-pressed` / `aria-expanded` 已为真时不点，不会把它关回去），再等行出现——命中「公网访问」里的行或 dsh 折叠卡里的行都不会落在没显示的地方；目标自己就是页签时（智能体的内置目录、Skills 的发现）同样写法，命中即选中该页签并高亮它。内置页清单目前覆盖：通用 / 桌面宠物 / 网络代理 / 快捷键 / 检查更新 / 内测功能 / 性能诊断 / Web 访问 / 11 个 CLI 引擎页逐行索引；CLI 的同一行在 11 个引擎页都有，命中按引擎分组（启用中的引擎本就排在前面）。列表类页（工作区、Skills、智能体）索引的是真正的设置项——分组/项目/已授权目录三段与各页页签；提示词页的列表与归档管理、用量、关于一样是纯用户数据，只靠页名进入，搜它们的内容是数据搜索、不是设置搜索。不翻译的文案（品牌名 `CC GUI`）走 `labelText`；平台不存在的行不入索引（Windows 独有标题栏、web 端不渲染的已授权目录与 Skills 页签）。匹配是「行标签 / 所在卡片 / 别名（`keywords`）」的大小写不敏感子串（卡片名可搜是有意的：`外观`、`行为` 是用户记得的名字，命中行的第二行正好解释了为什么命中），结果行标签里的 i18next 计数占位符会被去掉（`管理历史记录 ({{count}})` → `管理历史记录`）。页面往往是异步画出行的（要等设置读回），所以跳转后要等锚点出现再滚动（MutationObserver + 3s 放弃），高亮由行自己按当次锚点渲染，不会「跳到空处」；`prefers-reduced-motion` 下滚动改为瞬时。回归：`settings-search.test.ts`、`builtin-search.test.tsx`、`settings-shell.test.tsx`。
- **项目右键菜单要渲染插件注册的条目**：侧栏工作区行的右键菜单（`workspace-context-menu.tsx`）先出宿主内置项（新建 Worktree / 设置别名 / 归档…），再用分隔线接上 `workspaceMenuRegistry` 的插件条目（`ctx.ui.registerWorkspaceMenuItem`，权限 `ui:workspace-menu`）。宿主实现了注册与权限校验却不消费注册表，等于插件声明了权限也永远不出现在菜单里——这是 CCB「按项目单独启用」缺失的原因。三条硬约束：①**目标是右键那一行**（`{ workspaceId, archived }`），不是当前活动工作区，菜单不改变活动项目；②`label` / `visible` 在渲染期求值，语言切换或插件自身状态变化会给已打开的菜单重新贴标签，`compareByOrder` 排序（无 `order` 排最后、同序按 id）；③插件回调是外部代码——`label` / `visible` 抛错只丢该条目、`onSelect` 抛错或 promise reject 只记日志，图标另包 `PluginBoundary`（崩溃回落拼图图标），都不许把侧栏带崩。`label` 可以是 `{ text, status: { text, tone } }`：状态以括号小字跟在名字后，`tone` 只能选 `success` / `muted` 两个语义 token（插件选语气，不选颜色）。**只有插件条目时菜单照样打开**（开关入口不能因为宿主没传内置回调而消失）；最后一个插件在菜单打开期间卸载则菜单自行关闭，不留空浮层。回归：`workspace-context-menu.test.tsx`、`ai-chat-sidebar.test.tsx` 的插件条目用例。

- **界面缩放与字体设置一处存储、多处入口**：设置 → 通用 → 外观的「界面缩放」与状态栏 ± 按钮、缩放快捷键（⌘= / ⌘- / ⌘0，快捷键页可改）读写同一份 localStorage 百分比（`src/lib/zoom.ts`，50–200、步进 10），经 `ccgui:zoom-change` 事件互相同步，任一入口改动其余立刻跟上；应用走 Tauri 原生 webview zoom，重启由状态栏启动时重放。「界面字体 / 代码字体」（`src/features/settings/font.ts`）持久化在 AppSettings：`fontFamily` / `codeFontFamily` 存模式（空 = 「系统默认」，含旧 `system` 值，即内置 Inter / JetBrains Mono 加系统回退；`custom` = 上传的字体文件），`fontFile` / `codeFontFile` 存该文件绝对路径；应用方式是覆盖根元素 `--font-inter` / `--font-mono-source` 变量（聊天代码块与内置终端随 `--font-mono-source`，终端经 `ccgui:font-change` 事件热更 `term.options.fontFamily` 并重新 fit），bootstrap 首帧前从 localStorage 镜像预应用避免换字闪烁。自定义模式 = 下拉（只有「系统默认 / 自定义」两项）选「自定义」后右侧出现文件选择按钮，点击唤起原生字体文件对话框（TTF/OTF/TTC/WOFF/WOFF2），选中的文件经 Rust `read_font_file` 读取（校验字体魔数与 64 MB 上限，base64 回传）后用 FontFace API 注册为固定字节性家族名（`CCGUI Custom UI/Code Font`，重选替换旧 face），加载完成再触发 `ccgui:font-change` 让终端按真实字体重新量度；读取失败（不存在 / 过大 / 非字体）显示本地化错误并保留原选择，不落半成品设置。路径持久化：切回「系统默认」不清除已上传文件，再次选「自定义」直接重新应用（无需重选）；文件被移走/删除则静默回退到字体栈里的后备字体，设置不丢。旧 `system` 值与文件选择器之前的已安装字体名统一归并到「系统默认」（模式归一化，根变量随之移除），不再按字体名渲染。Web 访问模式不提供自定义（无原生对话框，且 web 桥不暴露任意文件读取），文件选择器与「自定义」选项只在桌面端渲染。宿主字体栈（`--font-sans` / `--font-mono` / `--default-font-family` / `--default-mono-font-family`）在 `theme.css` 的 `:root` 里额外以**无层**声明重推一次：插件 bundle 注入在 `@layer ccgui-plugins`（层序在 `theme` 之后），自带 Tailwind 构建的插件会输出 `--font-sans: var(--font-sans-host), …`，而它自己又声明 `--font-sans-host: var(--font-sans, …)`，两者成环使计算值为 guaranteed-invalid，preflight 回退到 `-apple-system`，界面/代码字体设置静默失效（kimi-lb 实测）；无层声明胜过所有 @layer，插件不能再改写宿主字体栈（`@theme` 里的同名定义仍保留，供实用类生成）。回归：`font-settings.test.tsx`、`builtin-search.test.tsx` 行索引用例、`plugin-ui-tokens.test.ts` 无层重推守卫、Rust `fonts::tests`。

- **聊天内容列宽是一档显式设置（宽幕布），不是自适应**：时间线、输入框、状态栏、附件行、排队消息、提问 dock 与计划 dock 共用同一条居中轨道（`src/features/chat/chat-column.ts` 的 `timelineColumnClass` / `centerColumnClass`——两个旧宽度 `max-w-[750px]`（时间线）与 `max-w-3xl`（输入框一侧）原样保留，缺开关时界面与以前逐像素一致）。设置 → 通用 → 外观的「宽幕布」开关（默认关）打开后整条轨道去掉宽度上限、占满所在窗格，只留窗格自身的 16px 内边距：文档列多宽由用户自己决定，不做「窗口够宽就自动变宽」的隐式行为，也不把默认改成全宽。开关状态走 `AppSettings.chatWideLayout` → chat store 的 `wideLayout`（`sessions.ts` 初始化时同步、`preferences.ts` 的 `setWideLayout` 落地），点击即时生效并随设置持久化；分屏时每一格按同一开关渲染（格子窄就自然收窄，没有第二种宽度）。回归：`MessageTimeline.test.tsx`（宽幕布用例）、`font-settings.test.tsx`（开关落设置 + 驱动 chat store）、`builtin-search.test.tsx`（设置行索引双向对齐）。

- **文件 Markdown 预览用 Streamdown 渲染**：`MarkdownPreview.tsx` 用 Vercel Streamdown（`mode="static"` + `code`/`math`/`mermaid`/`cjk` 插件），不再是裸 react-markdown 加手写标题样式（旧预览的 GFM 表格渲染成无边框纯文本）。它的 shadcn token（`bg-background`、`text-muted-foreground`、`border-border` 等）在 `src/styles/globals.css` 桥接到语义 token（`:root` 映射 + `@theme inline` 导出、`@source` 扫描 dist），暗色随 `.dark` 翻转，不另写 `dark:`。表格/代码块/图表的复制、下载、全屏按钮文案走 `files.markdown.*` i18n；外链一律 `openExternal` 交系统浏览器（内置 link-safety 弹层关闭，避免双重确认）；本地相对图片仍解析到 Markdown 文件旁的真实路径（`resolveMarkdownImageSrc`）。Mermaid 图滚入视口才渲染（IntersectionObserver 懒渲染），离屏留白是设计行为；编辑预览用 deferred 草稿整篇重解析，不逐键击卡顿。**正文链接必须自己带可见样式**：组件替换了 Streamdown 内建的 link 组件，它的 `text-primary underline` 类不会跟过来，所以锚点显式挂 `.md-preview-link`（`src/index.css`：绿色 `--md-link-color` + 点状下划线，与聊天 Markdown 的链接观感一致）；`https:` / `mailto:` 交 `openExternal`，相对 / 绝对文件路径按 Markdown 文件所在目录解析后开成编辑器页签（`resolveMarkdownLinkPath`），目录、无扩展名目标、`#anchor` 与未知 scheme 渲染为惰性文本——绝不让 webview 导航替换应用外壳。**预览支持 ⌘F 查找**：页头预览模式有放大镜按钮，快捷键与对话内搜索共用 `chatSearch` 动作（各面按可见性让位）；命中用独立 highlight 名的 Custom Highlight API 画（`src/features/files/markdown-search.ts`：全部黄色、当前项橙色，与对话搜索可同时开启互不覆盖），输入框是右上角共用的 `ContentSearchBar`，Enter / Shift+Enter 逐个跳转、Esc 关闭，命中计数实时显示。回归：`markdown-search.test.ts`、`TimelineSearchBar.test.tsx`、`tests/browser/markdown-preview.html`。
- **HTML 文件点开即渲染预览（桌面 `ccgui-preview` 协议 iframe + sandbox）**：`.html` / `.htm` / `.xhtml` 与 Markdown 走同一套头部「编辑 / 预览」切换，初始模式由 `src/features/files/editor-view-mode.ts` 的 `opensInPreview(name, !isWeb)` 决定，预览本体是 `HtmlPreview.tsx` 指向 `previewFileUrl(path)` 的 `<iframe>`。**不能用 Tauri 的 asset 协议**：`convertFileSrc` 把整条绝对路径编码进一个 URL 段（`asset://localhost/%2F…%2Findex.html`），文档的相对引用（`draft.css`、`<script src>`）会被解析成 `asset://localhost/draft.css` 而全部 404，页面只剩无样式裸 HTML——所以 `src-tauri/src/preview_protocol.rs` 注册了保留真实路径结构的 `ccgui-preview` 协议（每段单独解码；解码出 `/`、`\`、NUL 或 `.` / `..` 的段直接拒绝，不静默改目标），浏览器就能按目录解析同级资源、module script 与 `fetch`。访问范围沿用 asset 协议作用域（`$HOME/**` 减 deny，`Scope::is_allowed` 会 canonicalize 并跟随符号链接），响应带 `Cache-Control: no-store`（草稿靠刷新迭代，不许回放旧副本）与 `Access-Control-Allow-Origin: null`（sandbox 帧的不透明来源能用，带真实来源的远程页不匹配）；分支支持单段 Range（媒体拖动），多段 Range 按规范允许的方式忽略。sandbox 取 `allow-scripts allow-same-origin allow-forms allow-modals`——脚本、表单与 `alert` 能跑，但帧与宿主不同源、进不了 CC GUI 状态，顶层导航与弹窗始终被拦。`tauri.conf.json` 的 CSP 必须放行 `frame-src 'self' ccgui-preview: http://ccgui-preview.localhost`（缺了帧直接空白）。**预览渲染的是磁盘上已保存的内容**（与 Markdown 预览的实时草稿语义不同）：未保存改动靠头部「未保存」徽标与「保存」按钮提示，改完点「刷新」（§7 登记，点击重挂载 iframe）才可见；切到别的页签时 iframe 整个卸载（不在屏上的预览页不许在后台跑 rAF/定时器），切回来重新从盘上加载。**web 访问模式不提供 HTML 预览**（没有对应的原生协议），那里的 HTML 保持源码视图；作用域外的文件预览为空、源码视图与编辑不受影响。回归：`editor-view-mode.test.ts`、`preview-url.test.ts`、`HtmlPreview.test.tsx`、`FileEditorHeader.test.tsx`、Rust `preview_protocol::tests`。

- **内网访问自启、访问 IP 切换与端口/Token配置**：设置「远程访问 / 内网访问」（`WebAccessSection.tsx`）提供「随应用自动开启」滑动开关（`Switch`），开启时客户端启动即自动运行内网 Web 服务（后端持久化于 `AppSettings.web_access_auto_start`，前端启动时带兜底探测与自启保障）。运行态下提供「访问 IP / 网卡」下拉框（`Select`），通过平台原生接口（Windows `GetAdaptersAddresses` / Unix `getifaddrs`）动态枚举本机网络接口 IPv4 列表，优先置顶 Tailscale 虚拟网卡与 CGNAT IP（100.64.0.0/10），并列出物理网卡及 Localhost 回环地址；切换 IP 联动实时更新访问地址、复制内容与二维码，并在本地持久化所选偏好（`WEB_ACCESS_SELECTED_IP_KEY`）。支持自定义监听端口（`web_access_port`，留空为自动分配随机端口）与持久化鉴权 Token（`web_access_token`，支持一键「重新生成」）；服务运行中修改配置在卡片内展示重启提示与快捷「立即重启服务」动作，绑定失败时在界面显式展示端口冲突原因。回归：`WebAccessSection.test.tsx`、`web::tests::*`。

### 3.1 自动压缩：阈值控件与幕布行

- **上下文占用未知不显示 0%**：输入栏底部仪表尚无有效用量时显示「—」，屏幕阅读器说明「暂未获取上下文用量」；弹层同样不声称已用 0 或空闲 100%，仍保留刷新与压缩入口。空对象、全零占位报告不能覆盖已知占用，窗口容量单独上报时仍更新容量；有效的压缩后较小快照正常降低占用，不按历史最大值锁定。仪表使用最后一次请求的上下文快照，本轮计费与消息用量使用各次请求总和，结果事件的 `last_token_usage` 不得替代外层计费总量；`context_only` 压缩报告只更新占用，不重复进用量台账。容量优先采用实时报告，其次模型记忆、目录信息和兜底值。实现：`usage.ts`、`context-window-memory.ts`、`store/engine-events.ts`、`ai-chat-composer.tsx`、`agent-limits-card.tsx`；回归：`usage.test.ts`、`context-window-memory.test.ts`、`usage-accounting.test.ts`、`ai-chat-composer.test.tsx`。
- **Claude 账号额度与上下文分开**：Linux SSH Claude 会话的底部状态栏给独立额度入口，摘要明确显示「余 5h / 周」，窄窗口只保留图标。点击弹层展示账号共享的 5 小时、周额度剩余比例、重置时间、查询时间与套餐；进度条及读屏均按剩余比例解释，未知或已过重置时间的数据用「—」，不假定已恢复到 100%。首次打开、任务结束及查看详情时按需查询，前后端均合并同账号请求并缓存 60 秒，不持续轮询；窗口到期触发重新查询，手动刷新只绕过成功缓存。失败保留上次数据与具体提示，30 秒退避期间禁用刷新并显示可重试时间。前后端账号缓存最多 64 项。令牌只在 VPS 与固定 Anthropic HTTPS 额度接口之间使用，不返回 Mac、不跟随重定向；接口不可用时不推算余量。实现：`features/claude-usage`、`ConversationFooter.tsx`、`claude_account_usage.rs`；回归：`store.test.ts`、`ClaudeQuotaButton.test.tsx`、Rust 与 Python 额度测试。
- **压缩过程在时间线上只有一条灰线（幕布行），不是气泡**：宿主发出的 `/compact` 用户行（手动按钮或跨过阈值的自动压缩，`ConversationFooter.tsx` 的 `compactSession`）原位渲染为一行右对齐的灰色「‹ 正在压缩上下文 ›」（`MessageTimeline.tsx` 的 `CompactionCurtain`；`internal-rows.ts` 的 `isCompactCommandRow` 把该用户行转成 `curtain` 行）。它属于那条消息，压缩结束后继续留在历史里——用户要能回看「这里压缩过」；原先是用户气泡，已按同一提交移除。阈值触发压缩完成后 footer 自动补发的续接提示（`chat.autoCompactResume`）两语都按 `isResumeNudgeRow` 过滤，从不渲染，模型的回复才是可见部分。
- **响应校验解析请求别名**：Claude 的请求选择器（如 `opus`）保持原样交给 CLI；校验使用该次启动 CLI 注册表及渠道映射解析出的具体模型，卡片请求模型展示「选择器 → 具体模型」。供应商专属别名按对应注册表映射解析；远端运行不套用本机注册表。无法解析时展示「无法确认」并使用中性色问号，不作为不一致或通过；响应未上报仍显示「未上报」，无响应证据时隐藏徽标。实际具体模型或已上报档位不同仍使用黄色叹号。其他引擎及旧事件沿用原比较方式。
- **宿主压缩原位常驻，引擎回合中压缩挂尾部**：宿主自己发的压缩（`session.compaction.automatic === false`）已经有原位幕布行，时间线尾部不再重复画；引擎回合中自己触发的压缩（`automatic === true`，只有 `compaction` flag、没有消息行）把幕布行挂在时间线尾部（`MessageTimeline.tsx` 的 tail 槽位），flag 被引擎事件清除即卸载。
- **压缩期间尾部指示器让位，但槽位保留**：`session.compaction` 非空时 tail 槽位不再渲染 `AgentThinking` 波浪行（原位或尾部的幕布已经在表达进行中），`null` 占位而不是把槽位整个拿掉；`count = rows.length + (streaming || session.compaction ? 1 : 0)` 保证压缩轮与续接轮之间的空档幕布不闪断、结束后也不留悬空占位。
- **阈值控件按会话保存，无会话时可见但不可操作**：阈值数字输入（1–100 整数，`normalizeAutoCompactThreshold` 统一夹取与取整）与闪电开关在状态栏上下文弹层的用量卡底部（`AutoCompactControls`，`agent-limits-card.tsx`），存储键 `ccgui-next.chat.autoCompactBySession`（`auto-compact-context.ts`，默认阈值 `DEFAULT_AUTO_COMPACT_THRESHOLD = 80`），按 `sessionKey` 读写，换会话即换设置、新建待发会话的设置在拿到真实 sessionId 时随会话迁移。没有会话时两个控件保持可见（首启就能看到入口、卡片行高不跳）但禁用：`cursor-not-allowed opacity-50`，输入框走原生 `disabled`；开关**刻意不用原生 `disabled`**——原生禁用收不到 hover/focus，解释禁用原因的 tooltip 就永远读不到，所以用 `aria-disabled` + press 守卫（`agent-limits-card.tsx` 注释），tooltip 文案换成 `chat.autoCompactNoSession`（「新建或打开一个会话后可设置；阈值按会话保存」），可用态才显示「开启 / 关闭自动压缩」。
- **续接只回到原来那个会话标签页**：自动压缩完成后由 footer 发一次 `chat.autoCompactResume` 把任务接回去，条件收在 `shouldResumeAfterAutoCompact`：必须是阈值触发、同一个会话的标签页仍在 `openTabs` 里、压缩没有报错、用户没有按停止、没有排队消息、会话没有停在问答 / 审批 dock。压缩期间用户关掉了标签页就什么都不做，绝不回落到当前活动会话（`ConversationFooter.tsx` 的 `resumeAfterAutoCompact`，与 `refreshSessionUsage` 的「closed tab 不得回退到 active」是同一条约束）——那条续接指令是一条真实用户消息，落到别的会话会让它真的开始续作。

- **编辑精选轮播（市场首屏）**：首屏轮播是**编辑层**，数据来自索引仓的 `featured.json`（实现见 `src/features/plugins/hub/PluginSpotlight.tsx`）。三条硬约定：
  - **装饰数据不挡路**：文件缺失/坏掉，或某一行的 `id` 不在索引里，就整块/该行不渲染——不弹错误、不占位、不影响下方表格与筛选（`marketplaceStore.featured` 为空即整块 `return null`，索引本身的失败才进 `error`）。
  - **进度条就是计时器**：自动播放由 `theme.css` 的 `--animate-spotlight-progress`（6s，`scaleX` 不触发布局）驱动，`animationend` 才翻页。悬停、焦点进入、切到后台、`prefers-reduced-motion`（`useReducedMotion` + `motion-reduce:animate-none`）四路都作用在同一条进度条上，条停 = 翻页停，不存在「条停了还在翻」。手动翻页把进度条重新起跑（进度段 `key={index}`），不接力上一张的进度。
  - **封面素材链**：编辑封面 `image`（`object-cover` 铺满）→ 插件第一张截图（按原比例 `object-contain` 装帧，**不裁切**——索引里的截图从 3600×740 到 357×425 都有）→ 插件 `icon` → 品牌色首字块；任一环 `onError` 降一级，卡片永远画满。18 个已登记插件只有 6 个带截图，所以「没有图」是正常状态，不是错误态。
  - 文案压左侧压暗层（`from-black/85 via-black/55`）之上，对比度不依赖封面本身；轮播内键盘 ←/→ 翻页（做法同 `PluginScreenshotCarousel`），`aria-roledescription="carousel"` / `slide` + 非当前页 `aria-hidden`，圆点带「第 n 条精选：名称」。

## 4. 动作反馈

- **并发会话运行状态点**：侧栏、页签和收起的 worktree 聚合状态复用 `sidebar-thread-status`。运行中保持 0.92s 呼吸节奏，只动画 `transform` / `opacity`，光晕阴影保持静态，避免并发会话逐帧重绘阴影。退避重试与系统减少动态效果下保持静态蓝点，未读完成态保持静态绿点。浏览器回归：`tests/browser/concurrent-status.html`（1 / 6 / 12 会话，侧栏 + 页签，正常 / 重试 / 完成、亮暗主题和减少动态效果）。
- **大型过程组有界展示**：`ProcessDisclosure` 每页最多 40 条思考/工具条目，默认展示最新页；「上一页 / 下一页 / 回到最新」保留全部历史可访问。用户翻到旧页后，新增工具不抢回最新页；对话内搜索命中隐藏条目时展开过程并定位到对应页。大组或批量入场取消 blur/height/mask 动画，不裁剪思考或工具原文。
- **性能诊断入口**：底部状态栏「性能」与设置「其他 → 性能诊断」页的「查看性能诊断」按钮打开同一个弹窗（该页已从「社区与反馈」页移出，含说明与打开按钮）。默认开启，提供「自动性能诊断」开关并持久保存选择；关闭停止前端与原生采样、清空记录和预览，重新开启从新窗口开始。关闭前提示先导出需保留的证据；保存失败保留原状态并显示错误。前端与原生各保留最近 **5 分钟、最多 60 条**，记录仍仅在内存，重启清空。默认只读预览与「复制诊断摘要」使用不超过 **12,000 UTF-8 字节**的结构化摘要（峰值、前五进程、峰值附近采样与最严重阻塞附近采样）；「导出完整诊断文件」保留当前快照的全部数据，以紧凑 JSON 保存，不拼接旧报告。桌面选择保存路径，取消不提示成功，写入失败提示重试；Web 发起下载后仅提示已发起，不假称落盘成功。按钮生成前/操作中禁用，窄屏允许换行。保留隐私、单核与整机 CPU 区别及 WebKit 候选归属说明；原生不可用仍可复制前端摘要；剪贴板拒绝显示 `role="alert"` 并保留手动选择文本。弹窗使用 `ModalShell`、标准按钮与可滚动内容区，不新增刷新入口。
- **渲染性能面板（react-scan）**：设置 → 其他 → 性能诊断页内的独立开关，**默认关闭**，手动开启后即时生效并持久化。打包（生产）版只提供重渲染高亮与次数，不含单次渲染耗时（开发版 `pnpm dev` 才有）。react-scan 必须在 React/react-dom 首次导入前接管 instrumentation，因此入口 `src/main.tsx` 只做启动编排：先装轻量 devtools hook，再按开关决定是否加载 overlay，应用体经动态导入的 `src/bootstrap.tsx` 加载；`src/lib/react-scan.ts` 只在开关开启时拉取 react-scan 本体 chunk（未开启时只多取几 KB 的 hook chunk，开关无需重启即生效）。

异步动作必须让用户看到三件事：**正在进行**、**成功**、**失败**。失败要么有对号以外的显式反馈（错误文案 / 状态标记），要么保持原样不误导。

- **顶栏外部应用打开**：`HeaderOpenActions.tsx` 的固定按钮和更多菜单共用打开入口；启动失败用 `ModalShell` 展示目标应用及错误原因，允许关闭后重试，不静默吞掉失败。macOS 的 IntelliJ IDEA 使用应用包内 `Contents/MacOS/idea` 命令行入口，使已打开的项目复用窗口；其他预置应用继续使用 `open -a`。

### 4.1 刷新 / 重新加载：转圈 → 对号

参考实现：变更面板的刷新按钮（`src/features/git/ChangesPanelHeader.tsx`），公共实现：`src/components/base/action-feedback.tsx`。

规则：

1. **进行中**：动作图标转圈，动画用 `animate-refresh-spin`（`--animate-refresh-spin`，0.6s 一圈，linear infinite）。不要用 `animate-spin` 表达刷新反馈，也不要自定第二条时长。
2. **成功**：图标交叉淡出、绿色对号淡入 —— 对号色为 `text-notification-success-foreground`，停留 **900ms** 后淡回原图标。
3. **失败**：**不出现对号**，直接复位到原图标。
4. **至少转满一圈**：动作结束时若当前这圈没转完，等它转完再换对号（图标落回正方向，不会"歪着头"顶着对号）。这就是 `spin: true` 的含义。
5. **纯视觉反馈**：反馈容器 `aria-hidden`，按钮的 `aria-label` / `title` 保持动作名不变——刷新是否有新数据由界面本身说明，不需要播报。

接入方式（按动作有没有现成的 busy 状态选）：

| 场景 | API | 说明 |
|---|---|---|
| 动作返回 Promise，点击即发起 | `useActionFeedback({ spin: true })` → `start(action, isFailure?)` | `start` 会原样返回/抛出动作结果，接回调用方既有的错误链路；非抛错型动作（把失败写进 store）用 `isFailure` 报告失败 |
| 已有 store / props 的 in-flight 标志，或动作由别处触发 | `useRunningFeedback(running)` | 标志为 true 时转圈，落回 false 时给对号 |

```tsx
// 点击驱动
const refreshAction = useActionFeedback({ spin: true });
<button
  disabled={refreshAction.feedback === "running"}
  onClick={() =>
    void refreshAction.start(() => store.refresh(force), () => store.getState().error != null)
  }
>
  <ActionFeedbackIcon icon={RefreshCw} feedback={refreshAction.feedback} spin />
</button>

// 状态驱动
const feedback = useRunningFeedback(store.loading);
<ActionFeedbackIcon icon={RefreshCw} feedback={feedback} spin />
```

补充参数：图标不是 16px 时传 `iconClassName`（`size-3` / `size-3.5` / `size-[18px]`），需要忙碌态变色时传 `runningClassName`（如用量卡片的 `text-blue-500`）。

**不要这样做**：

- 自己写 `animate-spin` / 自定义时长 / 另一种成功表达（绿字、toast、换图标颜色）。
- 把**进度**当**刷新**：插件安装、文件上传这类有明确百分比的过程用 `Loader2` 进度语言，不用转圈+对号。
- **纯文本按钮套反馈**：报错态里的文字型"刷新"（`FileTreeBody`、`EditorPane`）保持文本形态，见 [§8](#8-待收敛)。
- 成功后按钮会立刻消失的场景硬凑对号（如插件"重新加载"成功后整行转为健康态）——按规则写，但不要为了看对号拖住状态更新。
- 与刷新无关的图标（更换密钥、重置、重发）借这套反馈。它们的语义是"变更/复位"，不是"重新读取"。

例外：变更面板的 **pull / push** 只用 `ActionFeedbackIcon`（不传 `spin`），即"点击 → 对号"——云朵图标转圈读起来像故障，不像进度。这类"一次性提交型动作"允许省略转圈，但成功/失败规则同上。

### 4.2 复制到剪贴板：Copy → Check

- 用 `src/hooks/use-copied.ts` 的 `useCopied(resetMs = COPY_FEEDBACK_MS)`，成功后图标换成 `Check`，**1500ms** 后复位。性能诊断需要显式处理复制失败，使用同一 `COPY_FEEDBACK_MS` 常量，成功反馈与卸载清理语义保持一致。
- 与刷新反馈的差异：复制没有别的成功信号，所以**可访问名一起改成"已复制"**（`aria-label` / `title`），刷新反馈则不改名。这是刻意的差别，不要强行统一。
- 复制按钮旁边有明文内容时（如密钥框），保留原布局尺寸与分隔符，只换图标。
- **非安全环境（局域网 HTTP）安全降级**：通过 `src/lib/clipboard.ts` 的 `copyText()` 或 `useCopied()` 复制，当 `navigator.clipboard` 因非安全上下文（如 `http://<ip>:<port>` 局域网 Web 桥）为 `undefined` 或调用失败时，自动降级到 `document.execCommand('copy')` 并安装全局 polyfill，避免抛出 `TypeError: Cannot read properties of undefined (reading 'writeText')` 导致界面崩溃。

### 4.3 桌面宠物（pet overlay）

- 窗口形态：独立透明置顶窗口（`src-tauri/src/pet_overlay.rs`），无边框、不进任务栏；仅宠物图像矩形接收点击（左键拖动＝`data-tauri-drag-region`，右键循环 50%/75%/100%/125%/150% 五档缩放并立即持久化；精灵为真实按钮，键盘 Enter/Space 与右键同效，焦点环走 `focus-visible`），状态气泡与其余透明区域保持鼠标穿透，不遮挡桌面操作。
- 状态气泡（`src/index.css` `.pet-bubble`）：有活动状态时显现，显隐动效 opacity 160ms / transform 180ms（`cubic-bezier(0.23, 1, 0.32, 1)`）+ blur 消退；`prefers-reduced-motion` 下仅保留 opacity 180ms，无位移与模糊。气泡为玻璃拟态（backdrop blur + 内高光），文本两行截断（会话名 + 状态）；状态文案走 `settings.petActivity*`，`aria-live="polite"`，无活动时 `aria-hidden`。
- 位置与尺寸记忆：拖动结束保存位置、缩放即时生效并持久化；恢复位置前校验仍在已连接显示器内，落出所有屏幕则用默认位置。宠物设置（开关/角色/导入/移除/尺寸）在设置 → 其他 → 桌面宠物；移除走 `ConfirmDialog`（danger），导入失败等后端稳定错误码经 `petErrorMessage` 映射为本地化文案。
- 多会话状态：气泡在并发会话间每 1.8s 轮播，刚完成的会话短暂展示「已完成」；失败/等待优先于运行中展示。

## 5. 加载、空状态与错误

- **后台 SSH 请求不突发建连**：同一主机与端口的 Linux 短调用最多并发 3 个，完整历史下载单独限制为 1 个并先等待下载位置，不能让排队下载占住所有短调用位置；Claude 启动前的 Linux 临时脚本上传也取得同一主机短调用位置，上传结束即释放，长任务本身不占位置。排队等待有 60 秒上限，与开始执行后的超时分开，失败与取消释放位置。上传的 stdin 写入、SSH 退出等待与 stderr 持续读取并发执行，共用一个 30 秒期限；诊断仅保留前 16 KiB，之后继续排空，超时终止该上传进程并保留失败阶段及已读诊断，不自动重试。Mac 的只读查询复用按主机、端口、用户与私钥配置隔离的 SSH 连接，空闲 60 秒后关闭；控制套接字仅当前用户可访问。不同工作区或身份不能绕过主机并发限制；写操作、WSL、Claude 长任务与交互终端连接独立。WSL 不加入主机队列，仍用原有 `tee` / `wsl.exe` 上传路径，共用上述并发读写与单一上传期限。同一项目的会话目录刷新合并并发请求，按扫描代数发布结果，旧批次不能覆盖新批次；成功仅清理该功能自己产生且仍在显示的旧错误，不清理其他运行错误。失败继续保留本地会话，不自动重放会修改远端的操作。SSH 终端不注册本地 Finder 路径探测器，避免对不能在本机打开的路径发起额外目录请求。实现：`engine/ssh_rpc_pool.rs`、`engine/wsl_transport.rs`、`store/ssh-sessions.ts`、`TerminalView.tsx`。

- **SSH 项目中的本机动作**：远程工作区隐藏本机编辑器 / Finder 打开、启动脚本与 Worktree 管理入口；聊天文件链接仍可在内置远程文件编辑器打开，终端入口始终可达，终端输出路径不触发本机 Finder。手工输入 `/ccgui-cua` 时明确拒绝并保留草稿，发送链同样拒绝且不得开启本机控制驱动；本机记忆 MCP 不挂载到 SSH 轮次。实现：`ChatPanelHeader.tsx`、`HeaderOpenActions.tsx`、`FileTreeContextMenu.tsx`、`FileLinkContextMenu.tsx`、`TerminalContextMenu.tsx`、`TerminalView.tsx`、`workspace-context-menu.tsx`、`store/messaging.ts`。

- **远程历史优先本地缓存**：打开会话先激活目标页签，通过 `load_cached_remote_session_page` 读取本机完整快照，无网络等待即可显示已缓存消息，然后后台更新远程记录。时间线不显示缓存状态横幅；「更新远程记录」收为顶部工具栏终端旁的 28px 图标按钮，窄窗口仍可见，仅在当前页签对应的 SSH 会话有缓存状态时显示。悬停与键盘焦点提示包含「本地历史缓存 / 正在从服务器更新 / 上次后台同步失败，已保留本地历史缓存 / 已更新，本地历史缓存可用」、缓存时间及具体失败原因；缓存时间表示快照保存时间，上次同步失败不表示当前聊天连接失败。按钮使用简短的本地化 `aria-label`、可见焦点环与屏幕阅读器状态，Enter / Space 可更新；更新期间禁用并转圈，失败用小标记提示，可点击重试，不清空旧消息。重复打开同一加载中或更新中的会话不重复请求。新请求开始时清除旧错误，不得清除期间新出现的运行错误、覆盖已完成的新回合或其他会话状态；后台刷新保留读者已打开的更早分页。向上翻页先读本机快照，有缓存时不发起远程请求，离线仍可翻页。实现：`store/sessions.ts`、`store.ts`、`ChatPanelHeader.tsx`、`HistoryCacheButton.tsx`；回归：`store.test.ts`、`HistoryCacheButton.test.tsx`。
- **离线会话目录**：按 SSH 主机、用户、端口和远程项目路径分区保存精简会话元数据，首启先发布本地目录和恢复页签，再后台读取远端清单。持久内容只含会话标题、路径和必要路由/状态，不含私钥路径、凭据、正文或预览；每项目最多 500 条、总计 2000 条，移除项目或更换连接后清理失配分区。连接失败且无当前内存目录时恢复持久目录；成功空清单、成功删除或归档清理对应目录，不能把失败当成空清单，也不能离线复活已删会话。实现：`ssh-session-catalog.ts`、`ssh-sessions.ts`；回归：`ssh-sessions.test.ts`、`store.test.ts`。

- 整块区域加载：`CenteredSpinner`；有内容但空：`EmptyState`（都来自 `src/components/base/empty-state.tsx`）。列表局部加载用行内文字或 `Loader2`，不要动辄整屏转圈。
- 行内错误：`role="alert"` 容器 + `text-text-error-primary` 文案 + 明确的下一步（重试 / 关闭）。
- 警告与失败要区分：可恢复的失败给重试入口，不可恢复的（未安装、平台不支持）给说明或跳转，不给假按钮。
- **远程桥刻意拒绝的命令不给假按钮**：被 `src-tauri/src/web/dispatch.rs` 明确排除的远程命令（如目录授权 `grant_root`，注释写明远端不得扩大文件系统授权范围），对应入口在 `isWeb` 下不渲染按钮、不显示“将授权…”预览，改为原因说明并只保留拒绝（`GrantCard.tsx`、`chat.grantWebUnavailable`）；桌面端保持完整动作。
- 进度类反馈（安装、更新、同步）用 `Loader2` / 文字百分比表达过程，与 [§4.1](#41-刷新--重新加载转圈--对号) 的刷新反馈互不替代。
- **Worktree 创建进度行三态**：创建对话框即交即走，进度在侧栏「WORKTREES」分组顶部的进度行表达（`WorktreeProgressRow.tsx`，事件 `worktree://create-progress` 驱动）——进行中：`Loader2` + 阶段文案（校验 / fetch / 创建 / 注册，逐段替换）+ ✕ 取消（取消杀进程组、半成品 worktree 清理、已建分支保留）；成功：行消失、worktree 子行出现（勾选了「创建后打开新会话」则先清场再在该目录开新会话）；失败/已取消：行保留为 `role="alert"`（失败原因按后端 `errorKind` 本地化分类：网络/fetch 失败给重试，分支或目录冲突给改名指引，非 git 仓库/PR 不存在给说明，稀疏检出规则排除全部文件（`sparse_checkout_empty`）给修复指引），行内「重试 / 关闭」。三态行高一致，状态切换不推动其他行。
- **崩溃绝不留白屏**：应用崩溃时必须显示可读原因，而不是纯白窗口。三层兜底：`index.html` 的启动占位 + 8s watchdog（bundle 加载失败或 React 挂载前崩溃时显示失败面板与重载，原因读 `localStorage` 的 `ccgui:last-crash`）；`src/lib/crash.ts` 的 `error` / `unhandledrejection` 全局捕获；`src/components/crash/AppCrashBoundary.tsx` 顶层 React 错误边界。渲染崩溃页（`CrashScreen`）必须给出**具体错误原因**、可展开的技术详情、`重新加载` / `复制错误信息` / `退出应用`（Web 不渲染退出）；非渲染的全局错误允许「继续使用」后关闭。环境性失败不弹崩溃页：浏览器自身噪音（`ResizeObserver loop`、`Script error`）与后台网络请求失败（断网 / 对端不可达，如更新检查的 reqwest `error sending request for url …`、fetch 的 `Load failed` / `Failed to fetch`）只记入诊断环，不上屏、不写入 `localStorage`（这类失败由各功能自身的错误状态表达，如更新行 / 浮层的失败与重试）。崩溃报告仅本地留存（内存环形 + `localStorage` 最近一条），不自动上传。
- **启动层资源必须是外部文件，`index.html` 里不得写内联 `<style>` / `<script>`**：打包链会给 `index.html` 里的每个内联标签打上 `__TAURI_*_NONCE__`，运行时把 nonce 追加进 `style-src` / `script-src`（`tauri-codegen` 的 `inject_nonce_token` + `tauri replace_csp_nonce`）；同一指令一旦出现 nonce，`'unsafe-inline'` 即被忽略，浏览器会拒绝**所有运行时创建的样式表**——插件的 `styles.css` 与 `ctx.theme.injectCss` 正是这样注入的，于是 v1.0.9 打包版静默丢掉全部插件样式，插件继续运行、无报错、不隔离（`plugin_list` 里 `lastError` 仍为 null），只有开发版看不出来（`pnpm dev` 的 HTML 不含 token，CSP 里没有 nonce）。样式在 `public/boot.css`、脚本在 `public/boot-watchdog.js`，两者都由 `style-src 'self'` / `script-src 'self'` 放行；回归：`tests/platform-build.test.ts`。

## 6. 破坏性操作

- 不可逆操作（删除、卸载、丢弃改动、断开授权）先确认：`ConfirmDialog`（`src/components/dialogs.tsx`），危险确认按钮用 `variant="danger"`。
- 由指针发起的行内破坏性操作可以用 `ConfirmPopover`，让确认贴近光标。
- 文案写清**后果对象**（删的是哪个文件/会话/插件），不写"确定吗？"。
- **退出应用**：窗口关闭按钮一律先确认（`src/lib/close-confirm.ts` 拦截 `CloseRequested`）；macOS 的 ⌘Q / 系统退出请求在存在进行中的会话时会被 `src-tauri/src/quit_guard.rs` 取消并复用同一弹窗，只有显式确认才销毁窗口退出，空闲时正常退出、不拦截。
- **删除 Worktree 分级确认**：`DeleteWorktreeDialog` 打开时预检 `git status`（未提交文件数 + 前两个文件名、未推送提交数）与 `git_branch_merged`（分支是否合入 base；同 tip 算已合入，squash 合入可能误报为未合入，文案如实说明），三项全干净只显示「没有未提交或未推送的改动」；有流式会话或活着的终端页签时额外提示。「同时删除本地分支」默认不勾（保留在仓库里），勾选后危险按钮文案升级为「删除 Worktree 和分支」；确认即关对话框，`git worktree remove --force` 后台执行（孤儿目录回落 `prune` + 提示手动清理，分支 `branch -d` 失败再 `-D` 仅显式勾选时），非致命尾巴（目录没删掉 / 分支被占用保留）走 `actionError` 横幅告知。会话历史保存在引擎侧，重新注册该目录会重新出现；侧栏登记在删除成功后移除。回归：`DeleteWorktreeDialog.test.tsx`。
- **父工作区移除/归档带级联提示**：移除或归档含 worktree 子项的父工作区时，`ConfirmDialog` 列出受影响分支再确认（移除：先移除子项登记再移除父行，磁盘目录不动；归档：一并归档）。要连目录删必须逐个走「删除 Worktree…」流程，父行移除不做目录级联。

## 7. 刷新入口清单

全项目的"重新读取"入口都登记在这里；新增一个入口就该在这里多一行。

| 入口 | 文件 | 反馈接入 | 备注 |
|---|---|---|---|
| Claude 账号额度刷新 | `src/features/claude-usage/ClaudeQuotaButton.tsx` | 弹层刷新图标 + 查询状态 | 显示 5 小时 / 周剩余额度与重置时间；按账号缓存合并，失败保留旧值，30 秒退避期间禁用并解释 |
| 远程历史更新 | `src/features/chat/components/HistoryCacheButton.tsx`、`src/features/chat/ChatPanelHeader.tsx` | 终端旁图标按钮 + 更新转圈 + 状态提示 | 时间线无缓存横幅；仅当前 SSH 会话显示，更新期间禁用，失败小标记与提示保留缓存及具体原因，成功提示「已更新」；重复请求去重 |
| SSH 连接与目录浏览 | `src/features/settings/SshWorkspacesCard.tsx` | `useActionFeedback({ spin: true })` | 显式连接、子目录与上级目录共用探测；成功显示实际 CLI / 目录，失败不显示对号并给错误；未挂载时忽略晚到的结果 |
| 变更（git）刷新 | `src/features/git/ChangesPanelHeader.tsx` | `useActionFeedback({ spin: true })` | **参考实现**；pull/push 只做 click → 对号 |
| 文件树刷新 | `src/features/files/FileTreeRow.tsx` | `useRunningFeedback(filesStore.refreshing)` | 入口在工作区根行（合成根节点），随该行悬停出现（同该行「添加到聊天」加号）；刷新可能由别处触发，故走状态驱动 |
| 插件市场索引 | `src/features/plugins/hub/PluginMarketView.tsx` | `useActionFeedback` | 图标按钮位于筛选工具条（分类 chips + 排序 + 搜索）右侧；`isFailure` 读 `marketplaceStore.error`。一次刷新同时重读索引与 `featured.json`（精选是软依赖：拉不到只清空轮播，不写 `error`） |
| 插件重新加载 | `src/features/plugins/hub/PluginInstalledRow.tsx` | `useActionFeedback` | 成功后该行转为健康态、按钮消失 |
| CLI 版本信息 | `src/features/settings/CliHeaderActions.tsx` | `useRunningFeedback(loading \|\| updating)` | 挂载时的自动探测同样转圈 → 对号 |
| 刷新用量 | `src/components/application/agent-limits/agent-limits-card.tsx` | `useRunningFeedback(refreshing)` | 带文字标签的卡片按钮，图标区放反馈 |
| 模型目录 | `src/components/application/ai-chat/engine-model-panel.tsx` | `useActionFeedback` | — |
| 浏览器刷新 | `src/features/browser/BrowserPane.tsx` | `useActionFeedback` | webview 无加载完成事件，对号 = 指令已下发 |
| HTML 预览刷新 | `src/features/files/FileEditorHeader.tsx` | `useActionFeedback({ spin: true })` | 重挂载 iframe 重新读盘；iframe 同样没有可等待的加载完成事件，对号 = 指令已下发 |
| 状态栏「立即同步」 | `src/components/application/app-status-bar/app-status-bar.tsx` | `useRunningFeedback(syncing)` | 进度由 `scan://progress` 事件驱动 |
| Skills 刷新 | `src/features/skills/InstalledPane.tsx` | `useActionFeedback({ spin: true })` | 一次动作同时重读已安装列表与更新信号；失败走行内 `role="alert"` |
| Bot「刷新上下文」 | `src/features/settings/agents-prompts/bot-prompt-preview.tsx` | **不加反馈**（就地换成一行说明） | 丢弃已冻结的提示词区块，下次发送重新拼装；语义是「放弃这次会话的快照」而非重读数据，所以不进转圈→对号那套，点击后按钮旁写明「已刷新，下次发送重新拼装」与「会让前缀缓存失效」 |
| MCP 刷新 | `src/features/mcp/McpSection.tsx` | `useActionFeedback({ spin: true })` | 重读配置清单与运行时分区；写入成功后也会自动重读 |
| `/mcp` 面板刷新 | `src/features/mcp/McpCommandPanel.tsx` | `useActionFeedback({ spin: true })` | 与设置页同一份 `mcp_inventory` 数据；写入成功后自动重读 |
| MCP 连接检测 | `src/features/mcp/McpSection.tsx`、`McpCommandPanel.tsx` | 行内状态徽标（`probe-ui.tsx`，检测中转圈） | 打开页面自动跑（复用 3 分钟内的结果），「检测全部」强制重跑；最多 4 个并行 |
| 报错态「刷新」 | `src/features/files/FileTreeBody.tsx`、`src/features/files/EditorPane.tsx` | **不加反馈** | 纯文本恢复入口，见 §8 |
| Worktree 状态采集 | `src/components/application/ai-chat/repo-tree.tsx`（`WorktreeGroup`） | **不加反馈** | 展开「WORKTREES」分组时后台刷一次 git status（复用 git store 30s TTL）与 `git_worktree_list`（locked/prunable），没有用户发起的「刷新」按钮；徽标随状态自然更新 |
| 更换密钥 | `src/features/settings/WebAuthCard.tsx` | **不加反馈** | 语义是"轮换"不是"刷新" |
| Git 任务管理「刷新」 | `ccgui-plugin/git-tasks/main.js` | `useRunningFeedback` 等价实现（0.6s 转圈 → 900ms 对号，失败复位） | 独立 ESM 插件（`exec:gh` / `exec:git`），不导入宿主 hook；重读当前预设下的议题 / PR 与预设计数。缺省不接入新的宿主依赖 |
| Git 任务管理「重新读取工作区与仓库」 | 同上 | **不加反馈** | 弹层内的菜单项，点击即关闭入口（同「重新加载」成功后按钮消失一类）；加载态由选择器自身的分组转圈与计数表达。重读 `ctx.workspaces.list` + git remote 解析 + `gh repo list` |
| 接力引擎列表 | `ccgui-plugin/ccgui-plugin-plan-execute-relay/main.js` | 异步动作期间禁用，失败行内告警 | 独立 ESM 插件的文本动作；不导入宿主私有反馈 hook。刷新仅重读可用引擎、渠道名和模型，不触发模型请求 |

注：Worktree **创建进度行不登记**在本清单——它不是「重新读取」入口，而是一次性任务的状态表达（进行中 → 成功/失败），用 §5 的进度语言（`Loader2` + 阶段文案），不存在「再刷一次」的语义；其失败行的「重试」是重新执行创建动作，同样不是刷新。

## 8. 待收敛

按"出现第二次同类问题就动手"的节奏处理，处理完把条目移出本节并写进正文。

- **`prefers-reduced-motion` 下的关键帧动画**：`ActionFeedbackIcon` 的转圈、`CenteredSpinner` 与各处 `Loader2` 目前仍会转动（过渡类已有 `motion-reduce:transition-none`）。目标：关键帧动画加 `motion-reduce:animate-none`，忙碌语义改由 disabled 态 + 文案承担。
- **两个纯文本"刷新"**（`FileTreeBody` 根目录报错、`EditorPane` 文件读不到）：要么补成图标+反馈（需要先给它们合适的按钮容器），要么确认为刻意的文本形态。目前按"不加反馈"登记在 §7。
- **反馈时长常量分散**：刷新时长在 `src/components/base/action-feedback.tsx`（600 / 900ms），复制在 `src/hooks/use-copied.ts`（1500ms）。如果出现第三处，抽成统一的动效常量，并回到本文登记。
