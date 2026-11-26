# 文件组织规范

当前实现按本机标签划分子工作区. 标签位于队伍对应的本机状态目录，旧版名册中的空间作为初始标签. 新建队伍在引导结束后从偏好配置标签. 工作目录保持普通文件夹结构，按相对路径对应队伍仓库中的共享文件. 设备的默认检查间隔为 30 秒，可在偏好调整. [同步设计与验证](implementation.md#session-driven-synchronization)


当前队伍 Git 协议为 v2. 操作信息写进 commit message，文件版本由 Git 保存. 下文涉及事件文件的约定仅用于读取 v1 仓库. 正文待办和子任务按当前进度规则汇总. [当前 Git 协议](git-commit-format.md) · [进度规则](implementation.md#inline-checklist-revision-and-independent-review)


蓝图 0.3 · 已评审的目录约定 · 队伍文件格式 v1

开发者在源码仓库里增加功能。客户端将已发布的协作数据写入队伍仓库，把身份、尚未送达的操作和恢复信息写入本机状态目录。队员实际编辑代码、论文的目录由每队在本机单独配置。下面的路径都相对于各自的根目录。

## 源码放在哪里

首个功能开始时建立一个 Cargo 包 `synod`。库承载领域与用例，二进制负责组装。下图是后续实现采用的路径。本次落地文档和测试样例，Rust 文件随对应功能建立。[Cargo 包目录约定](https://doc.rust-lang.org/cargo/guide/project-layout.html)

```text
AGENTS.md
README.md
docs/
  initial-design.md
  file-layout.md
  git-commit-format.md
  client-session.md
Cargo.toml
Cargo.lock
src/
  main.rs
  lib.rs
  mail/                 mod.rs, letter.rs, event.rs, board.rs, dependencies.rs
  workspace/            mod.rs, sync.rs
  application/          mod.rs, letters.rs, workspaces.rs, ports.rs
  session/              mod.rs, host.rs, client.rs, protocol.rs, transport.rs
  adapters/
    mod.rs
    git/                mod.rs, operations.rs, commit_message.rs, drafts.rs, history.rs, workspaces.rs
    local/              mod.rs, state.rs, outbox.rs, workspaces.rs
  tui/                  mod.rs
  mcp/                  mod.rs
tests/
  fixtures/v1/
    team/
    history.toml
```

| 新增的行为 | 实现位置与边界 |
| --- | --- |
| 信件事件、全队标题唯一性、公告板视图 | `mail/` 定义领域类型并重放事件。去重与依赖校验供两个入口共同使用。 |
| 手动或干净时同步的决策 | `workspace/sync.rs` 处理基线与本地差异。磁盘应用和中断恢复在 `adapters/local/workspaces.rs`。 |
| 寄信、勾选或发布子目录 | `application/` 表达用例并拥有所需的 `DraftStore`、`TeamHistory`、`WorkFiles`、`Outbox` 接口。Git 和本机文件适配器实现这些接口。 |
| 同机两个入口共同办事 | `tui/` 和 `mcp/` 使用 `session/client.rs`。`session/host.rs` 持有唯一的应用实例、身份位和写队列。`protocol.rs` 定义请求及结果，`transport.rs` 管理本机连接与进程所有权。 |
| 新增 Git 操作 | `adapters/git/` 是统一的 git-adapter。`operations.rs` 独占 Git 进程调用、提交与检出。`commit_message.rs` 编解码版本化 TOML。草稿、历史、子树模块把经过解析的参数交给操作模块。 |

依赖从 TUI/MCP 经会话客户端、本机会话宿主进入应用层，再进入领域或调用应用层拥有的接口。`main.rs` 在会话宿主中装配具体适配器。内部单元测试随对应模块，公开 API 的集成测试放在 `tests/`，共享队伍样例放在 `tests/fixtures/`。源码与开发提交遵循项目的[工程规范](../AGENTS.md)。

## 一支队伍的 Git 仓库

```text
team.json
letters/<letter-id>/
  envelope.json
  body.md
  attachments/<attachment-id>
  events/<event-id>.json
workspace/<space-id>/<relative-path>
```

| 路径 | 创建者与写入时机 |
| --- | --- |
| `team.json` | 建队时建立，记录 `schema_version`、`team_id`、名册 `members` 和空间注册表 `spaces`。名册区分人和助手，空间用稳定编号映射到展示名。 |
| `envelope.json`、`body.md` | 草稿作者在自己的分支编辑。信封记录 `letter_id`、`from`、`to`、`cc`、`title`、`initial_dependencies` 和 `attachments`。缺省 `cc` 读作空名单。寄出后固定原件。 |
| `attachments/<attachment-id>` | 保存附件原始字节，原文件名在信封的 `attachments` 中。路径由编号生成，附件与信一起封存。 |
| `events/<event-id>.json` | 寄出时新增 `LetterSent`，勾选时在原信下新增 `MarkedCompleted`。事件文件只追加，身份和带时区的操作时间随事件保存。 |
| `workspace/<space-id>/` | 队员发布该子空间时写入。空间内保留代码、论文等文件原有的相对目录结构。 |

队伍树和信封采用 `team.json` 声明的格式版本。事件文件和 Git 提交消息分别携带各自的版本。没有附件的信可以省略 `attachments/`，草稿在寄出前也可以没有 `events/`。已登记的空子空间可从 `team.json` 在本机创建，第一次发布文件时才在 Git 树中形成目录。

文件夹用稳定编号命名，草稿改标题、附件重名、成员改显示名都不会改变引用。每个编号须是一个可移植的路径段，拒绝绝对路径、分隔符、`.`、`..` 和平台保留名称。工作文件可有多级相对目录，逐段检查边界与符号链接。附件显示名只用于展示。全队查重、当前进度及 To/CC 筛选从主线还原，各客户端可自行建立缓存。

云草稿引用为 `refs/heads/drafts/<actor-id>/<letter-id>/<copy-id>`，沿用主线的信件目录结构。保存只提交这封信的草稿内容。寄信时主线接纳原件和一个寄出事件，后续状态操作每次只向该信追加一个事件。已寄信对本队所有成员可读，To/CC 只用于筛选与提醒。[队伍样例](../tests/fixtures/v1/team)

## 一台客户端的本机目录

```text
<system-config>/synod/
  client.json

<state-root>/
  runtime/
    owner.lock
    endpoint.json
  teams/<team-id>/
    repo.git/
    drafts/<actor-id>/<letter-id>/<copy-id>/
    outbox/<operation-id>/
      intent.json
      message.toml
      payload/
    worktrees/<attempt-id>/
    spaces/<space-id>/
      state.json
      recovery/<operation-id>/
        journal.json
        before/
        after/
    notices/<actor-id>/<event-id>

<work-root>/
  <space-id>/<relative-path>
```

启动配置解析出规范化的 `state-root`，并为每支队伍登记一个可编辑的 `work-root`。两类根目录互不包含，队伍工作目录也彼此独立。以空间编号 `paper` 为例，远端 `workspace/paper/figure-3.tex` 对应本机 `<work-root>/paper/figure-3.tex`。

| 位置 | 用途与寿命 |
| --- | --- |
| `client.json`、`runtime/` | 配置保存队伍绑定及人类、AI 两个独立身份位。凭据交给系统凭据机制。运行目录供同一客户端的唯一会话选主和发现本机端点使用。 |
| `repo.git/`、`drafts/` | Git 对象库、本地引用和长期编辑中的草稿工作副本。未云存的编辑或未发布的对象属于用户工作，恢复时保留。 |
| `outbox/` | 每次发布动作在网络调用前写入稳定编号。意图保存原身份、队伍、目标、基线和增改删清单，`message.toml` 是要发出的原文，`payload/` 保存操作需要的冻结字节。先在同卷临时目录完整写入并持久化，再原子移入 outbox，重试读取同一份意图。 |
| `worktrees/` | git-adapter 为一次尝试建立的隔离工作副本，成功或恢复后可释放。待投递数据的寿命由 outbox 管理。 |
| `spaces/`、`notices/` | 每个空间存自己的同步策略、已应用的主线版本和子树标识。恢复清单与新旧文件先落盘，整次应用成功后才推进基线。提醒凭据按队伍、身份、寄出事件编号去重。 |

后台只抓取指定队伍引用。空间基线和未完成操作仍需用到的 Git 对象由本机引用保活，推送使用明确的草稿或主线引用。队伍仓库只接纳上节规定的业务路径。改变本机同步偏好、读取提醒、切换 TUI 面板均只影响本机。

## 从样例还原一次交接

`tests/fixtures/v1/team/` 是比赛结束时的文件快照。旁边的 `history.toml` 是测试输入，列出每一步的目标分支、要写的文件、可选的草稿父提交，以及完整的 TOML 提交消息。按 `step` 顺序构造临时 Git 历史，文件内容从 `team/` 读取。真实的队伍仓库以 `team/` 的内容为根。[重放清单](../tests/fixtures/v1/history.toml)

从空仓库开始，在 `main` 建立首条提交。首次遇到草稿分支时从当时的主线检出，已存在的分支从各自的头继续。带 `merge_from` 的步骤先准备与该草稿的两父合并，再从样例复制 `write` 所列文件。把 `message` 字符串原样作为提交消息，推送到该步指定的分支。

| 动作 | 文件与引用的变化 |
| --- | --- |
| 写信、存草稿 | 会话为作者打开本机草稿工作副本。保存操作冻结原件，git-adapter 提交并更新远端草稿引用。 |
| 寄信 | 取最新主线重验全队标题和依赖，以该主线和已保存草稿为双亲，一次发布原件与 `LetterSent`。 |
| 勾选完成 | TUI 或 MCP 递交同一用例。会话排队并确认 `MarkedCompleted`。正文、附件及另一封信的树保持原样。 |
| 更新工作区 | 仅读写目标 `workspace/<space-id>/` 与该空间的本机基线。队友编辑 `code` 时仍可接收 `paper` 的更新。 |

## 验收

按清单重放后，远端 `main` 的文件路径和字节应与 `team/` 完全一致。沿第一父提交链应读到一次建队、两封信、两次完成和两次工作区发布。草稿提交在寄出合并后可从第二父追溯，公告板只按主线接纳的事件重放。统一 Git 写入与同机会话竞争分别按[提交协议](git-commit-format.md)和[客户端会话](client-session.md)核对。

### 0.3 独立评审记录

| 评审发现 | 修订与验证 |
| --- | --- |
| 最终文件树丢失草稿父提交以及事件被接纳的次序 | 重放清单位于队伍目录外。在临时远端读回了 9 个提交对象、2 次两父合并和 7 个主线提交，重新克隆得到的 12 个文件与样例逐字节一致。 |
| 默认合并会自动生成普通文本消息，草稿另存还可能改变已冻结操作的目标 | git-adapter 统一准备合并和结构化提交，副本有独立且可重试的派生意图。演练覆盖五种 TOML 消息的实际 Git 往返，另用六个反例检查字段、种类、时间和版本。 |
| 两个进程同时启动或旧端点残留时，可能分别取得写入权 | 通过系统锁选出唯一宿主。Unix 域套接字演练中，两个候选只选出一个所有者，两个前端进程同时勾选只产生一次远端完成事件。TUI 断开期间，另一个入口和重新连接的 TUI 读到同一结果。 |
| 远端已接受写入而宿主在本机确认前退出 | 在临时 Git 推送后强制结束演练宿主。下一代宿主接管旧端点，按原意图核对已发布事件，重试仍只读到一次完成。 |

这些结果来自格式样例和临时进程演练。后续 Rust 实现仍需执行 A10、A11 的端到端测试，并在 Windows 上验证命名管道与进程所有权。
