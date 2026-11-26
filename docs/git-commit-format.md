# 队伍仓库的 Git 提交协议

协议 v1 · 适用于 SYNOD 为队伍数据仓库生成的全部提交

## 消息原文

Git 提交对象的**整条消息**是一份 UTF-8 编码的 TOML 1.0 文档，以 LF 换行并以换行结束。写入器首行写 `kind`，空一行后写其余字段，方便直接看 Git 日志。解析器读取完整消息，按字段名解析。源码仓库中由开发者撰写的提交使用工程规范中的 Conventional Commits。[TOML 1.0](https://toml.io/en/v1.0.0)

```toml
kind = "letter.event"

schema_version = 1
team_id = "team-aurora"
operation_id = "op-008"
actor_id = "member-alice"
occurred_at = 2026-11-26T15:42:00+08:00

[target]
letter_id = "letter-042"
event_id = "evt-018"
event_kind = "MarkedCompleted"
```

每条消息都必填上述六个顶层字段。`kind` 决定可用的目标字段。`actor_id` 取自当时绑定的身份位，`occurred_at` 为含偏移量的 TOML 日期时间。`operation_id` 在第一次入队时确定。一次寄信如需先存草稿，保存和发布各有一个稳定操作编号。重试沿用各自的编号、时间和输入，即使发布时采用了新的主线父提交。

| `kind` | `[target]` 必填字段 | 引用与允许变更 |
| --- | --- | --- |
| `team.configured` | 无目标表 | `main`，建队或更新时只写 `team.json`。建队允许根提交。 |
| `draft.saved` | `letter_id`、`copy_id` | 作者对应的草稿引用。差异限于本信的信封、正文、附件。 |
| `letter.sent` | `letter_id`、`event_id` | `main`，增加指定的封存原件和一个 `LetterSent` 事件。第一父是刚校验的主线，第二父是所寄的草稿版本。 |
| `letter.event` | `letter_id`、`event_id`、`event_kind` | `main`，只新增对应事件文件。进度、完成、增减依赖使用各自的事件类别，载荷保存在事件 JSON 中。 |
| `workspace.saved` | `space_id` | 个人分支 `work/<actor-id>/<space-id>`，保存所选子工作区。 |
| `workspace.published` | `space_id` | `main`，新增、编辑或删除的路径均在该子空间内。 |

Git 消息描述一次仓库操作。事件 JSON 记录信件的业务事实。发布和读取时核对消息所指的队伍、目标和实际文件变化。涉及事件时，信件编号、事件编号、类别、操作者和操作时间也须与事件一致。重放公告板按主线第一父顺序读取已验证事件。操作时间用于显示，Git SHA 用于定位某次实际提交。

草稿因并发编辑需要另存时，会话先持久化与原动作绑定的副本编号及派生保存意图，再发布该副本。原动作重试可取回这一份结果。两台设备同时寄出同标题的不同信时，仍按最新主线的全队标题索引裁决。

## 唯一的 commit / checkout 入口

`src/adapters/git/operations.rs` 负责启动 Git 进程，掌握创建提交和检出工作副本的所有路径。`commit_message.rs` 负责在类型化的 `TeamCommit` 与 TOML 间转换。应用层提供业务意图，草稿、历史和工作区适配器提交给操作模块。TUI 与 MCP 向统一会话发命令。[源码目录](file-layout.md)

- **提交**接收已经准备好的变更、预期父提交和类型化消息。先核对允许变更的路径，再以 `git commit --cleanup=verbatim -F -` 从标准输入写入消息。推送前读回真实提交对象，验证消息、树和父提交。Git 钩子拒绝或改写出不符协议的结果时，保留本机意图并报告失败。[Git 提交选项](https://git-scm.com/docs/git-commit)
- **检出**接收类型化目标及隔离工作目录。新建草稿、续写草稿、检出已知主线或准备重试都走该入口，包括 `switch` 和隐式检出的 `worktree add`。切换工作副本本身只产生本机状态变化。
- **寄信合并**由同一模块用 `--no-ff --no-commit` 准备两父提交的树，加入寄出事件后走上述提交入口。先确认草稿内容及主线版本，再进行普通快进推送。远端前进时重验并重建候选。[Git 合并选项](https://git-scm.com/docs/git-merge)

读写边界拒绝缺失字段、字段类型错误、无偏移时间、目标类型不符、事件与树不匹配、未知种类或未知协议版本。同一次动作已按相同内容发布则确认成功，编号相同而内容不同则保留现场。遇到无法验证的历史，客户端保留最后一次可验证的视图并指出相关提交。凭据、文件正文和附件字节均有各自的存储位置。

## 验收

测试应从真实 Git 对象中取出消息，逐条解析，并核对引用、父提交和相对于第一父的路径差异。样例覆盖五种提交，以及一次通过两父合并寄信的完整路径。初次实现应在 git-adapter 的测试里覆盖格式错误和 Git 失败，并检查应用的提交和检出调用全部经 `operations.rs`。[提交消息样例](../tests/fixtures/v1/history.toml)
