# curvine-tui UI 重设计蓝图 (v2)

> 本文件是开发的唯一依据。目标：在保留全部现有功能的前提下，把交互从
> "tab 平铺 + 死记按键" 升级为 "上下文专注 + 可发现的操作"。
> 参照：lazygit（操作菜单、信息行）、k9s（上下文头、整屏详情）、yazi（扁平文件浏览、
> 多选）、btop（底部按键指引）。

## 0. 工程约束（必须先读）

- 仓库：/Users/infmax/CodeSpace/curvine-tui（paseo agent 的工作目录即此仓库的 worktree）
- **没有可用的真实集群**。所有验证走 hermetic 测试：
  - `cargo test`（当前 65 个单测 + 7 个 live 测试会自动跳过——live 需要
    CURVINE_TUI_LIVE=1，不要设它）
  - `cargo clippy --all-targets` 必须零警告（自身代码；curvine-937d* 的警告可忽略）
- 现有测试会因布局改动失败，**必须同步更新**，禁止删测试来"通过"
- 提交规范：`feat(ui): ...` / `fix(ui): ...`，git 身份已配好（lzjqsdd）
- 动画系统保留（springs、状态淡出）；本轮只动布局与交互
- 圆角边框用于"有内容的块"；单行信息条（header/footer/breadcrumb）**无边框**
- 图标系统 `src/ui/icons.rs` 保留，新场景按需取用

## 1. 全局骨架（src/ui/mod.rs）

```
┌────────────────────────────────────────────────────────────┐
│ ❖ curvine-tui   ● localhost:8995   1 worker   ↻5s    [1行, 无边框] │
├────────────────────────────────────────────────────────────┤
│                                                             │
│                      主区域（上下文相关）                        │
│                                                             │
├────────────────────────────────────────────────────────────┤
│ 状态消息(左, 4s 淡出)                         ↑↓ move  d desc …│ [1行, 无边框] │
└────────────────────────────────────────────────────────────┘
```

- **header**（替换现在的 3 行 tab 栏）：
  - 左：`❖ curvine-tui`（accent 加粗）+ 空格 + 面板名（当前上下文）
  - 中：集群信息——`● localhost:8995`（圆点颜色=健康）、`1 worker`、`↻ 5s`
    （下次自动刷新倒计时）。无集群数据时显示 `○ disconnected`
  - 右：panel 切换提示 `1-8 切换 · : 命令`
  - 颜色：背景 Normal，左边 accent，中间 muted，全部用 Span 拼接
- **footer**（合并现有 status 行 + hints 行）：
  - 左：状态消息（保留现有淡出动画，逻辑不动）
  - 右：面板上下文按键提示（现有 `render_hints` 的字符串，去掉现在占用的
    独立行，右对齐）
  - 实现：一个 `Paragraph`，Line 内左 span + spacer + 右 span
- **panel 切换**：数字键 1-8 保留；Tab 保留。`Panel::ALL` 不变。
- 主区域高度 = 全部剩余空间（constraints 从 `[3, Min, 1, 1]` 改为
  `[Length(1), Min(3), Length(1)]`）

## 2. 通用操作菜单（新文件 src/ui/menu.rs + src/app.rs 支持）

lazygit 的灵魂。任何面板按 `x` 打开上下文操作菜单（除 Files 里已选中多个时
见 §3）。

- 结构：居中竖排列表，每行 `[key] label`，key 用 accent 色，label muted；
  选中行反色。标题 = 当前选中项名（截断到 24 字符）
- 交互：`j/k` 或 `↑/↓` 移动，`Enter` 执行，`Esc` 关闭，**直接按 key 也执行**
- 状态：`App.menu: Option<Menu>`，`InputMode::Menu`；菜单不阻塞动画帧
- 数据：每个面板提供 `menu_items(&self) -> Vec<MenuItem>`，
  `MenuItem { key: char, label: String, action: MenuAction }`，
  `MenuAction` 是枚举，在 app.rs 的 reducer 里统一分发（不要在每个面板
  塞执行逻辑）

各面板菜单内容（key 沿用现有绑定，等于把现有按键"公开化"）：

| 面板 | 菜单项 |
|---|---|
| Files（文件） | `d` 详情 · `Del` 删除 · `F` 释放缓存 · `p` 上传 · `r` 刷新 |
| Files（目录） | `Enter` 进入 · `Del`/`D` 递归删除 · `r` 刷新 |
| Files（有标记） | `Del` 删除 N 个标记 · `F` 释放 N 个 · `u` 清除标记 |
| Nodes | `d` 详情 · `r` 刷新 |
| Mounts | `n` 新建挂载 · `u` 卸载 · `r` 刷新 |
| Jobs | `r` 刷新 ·（有 running 任务时）`c` 取消 |
| Transfers | `x` 取消选中 · `c` 清空已结束 · `p` 上传 · `g` 下载 |
| Fsck | `s` 扫描 · `x` 中止 · `f` 目标=当前目录 · `r` 重扫 |
| Bench | `b` 运行 · `p` 切换 profile · `t` 切换线程 · `d` 切换时长 |

`?` 帮助弹窗保留（更新内容），菜单是它的轻量替代。

## 3. Files 面板重设计（src/ui/files.rs）

```
[无边框]   /data/warehouse/                     filter 3/340  ●2 已标记
[表格]    name(size=最大)  mode owner size modified repl
[无边框]   ● file  -rw-r--r--  4.0 MiB  etl:data  2024-01-01   d 详情
```

- 第 1 行 breadcrumb：**无边框**。左：路径（accent）；右：filter 状态 tag、
  `N/M` 可见/总数、已标记数 tag
- 表格保持现有列（icon/name、mode、owner、size、modified、repl），
  `Constraint::Min` 给 name
- 最后 1 行选中信息：**无边框**单行，内容保持现有（icon、类型 tag、
  mode、size bar、owner:group、mtime、replicas），追加 `x 菜单` 提示
- **多选**（新交互，核心）：
  - `Space`：标记/取消标记当前行；标记行行首符号变为 `● `（替换 `▶ `），
    未标记的选中行仍是 `▶ `
  - `a`：标记全部可见；`u`：清除全部标记
  - `App.files.marked: Vec<String>`（存路径，防重名混淆）
  - 有标记时，删除/释放确认框显示数量："删除 3 个标记项？"
  - 导航在标记存在时不清空标记；`Esc` 清空标记
  - 批量删除/释放的确认与执行逻辑复用现有 `PendingConfirm`/`PendingAction`，
    扩展为支持 `Vec<String>` 路径
- 按键调整：`Space` 标记、`a` 全选、`u` 清标记、`x` 菜单；其余不变
- 标记行样式：name 列加 `● ` 前缀用 warn 色，其余同普通行

## 4. 整屏详情视图（替换 describe 弹窗）

- 新状态：`App.detail: Option<DetailView>` + `InputMode::Detail`
- `d`（Files/Nodes）打开；`Esc` 或 `q` 返回；详情不阻塞其他面板切换
- **FileDetail**（Files 的文件，非目录；目录仍弹 describe 弹窗）：
  - 上 1/3：kv 区（path、mode、owner、size、replicas(configured)、modified）
  - 下 2/3 左右分栏：左 = "blocks by offset" 表（block_id、size、offset、
    ●avail/total、state）；右 = "distribution by worker" 表（worker、
    blocks、bytes、share bar、百分比）
  - 数据复用 `App::files_describe` 的异步获取逻辑与
    `aggregate_by_worker`，只是渲染目标换成整屏
- **WorkerDetail**（Nodes）：整屏 kv 表（endpoint、ip、hostname、rpc/web
  port、worker id、state、version、weight、capacity、used、free、occupancy、
  blocks）+ storage 分类表
- describe 弹窗仅保留给：目录（Files）、umount/delete 确认（confirm 不变）、
  表单（mount/transfer 不变）

## 5. `:` 命令行升级（src/app.rs execute_command 附近）

- 命令输入期间（InputMode::Command），footer 行上方再渲染 1 行提示
  （无边框）：前缀匹配的命令列表。命令表：
  `ls cd mkdir rm mv put get mount umount load fsck bench panel refresh help quit`
- `Tab`：补全为第一个匹配命令 + 空格
- `↑/↓`：历史记录（最近 20 条，`App.command_history`）；`Enter` 执行后入史
- 匹配规则：`ls` 前缀即可；无匹配时提示行显示 `✗ 无匹配命令`

## 6. Fsck 简化（src/ui/fsck.rs）

- 删掉中间的 "verdict strip"（那排 `● 120 files ● 340 blocks …` 计数器）
- 布局改为 3 段：verdict gauge(3) / summary(8，把 files/blocks/replicas/
  under-replicated/empty/errors 全部收进 kv 行) / unhealthy 表(剩余)
- 表格加 `↑↓` 行选择，`Enter` 对坏文件打开 §4 的 FileDetail

## 7. Dashboard 微调（src/ui/dashboard.rs）

- 结构不变（左 cluster 右 workers），底部最近事件保留在左列
- 去掉列内多余空行；容量条与 namespace 合并间距紧凑
- workers 表加行选择（`↑↓`），`d` 打开 WorkerDetail（与 Nodes 一致）

## 8. 交付清单（按序完成，每步保持测试绿）

1. 全局骨架 header/footer（§1）+ 更新所有渲染测试
2. `x` 操作菜单（§2）+ 各面板菜单项 + 菜单渲染测试
3. Files 多选 + 批量操作（§3）+ 交互测试（标记/全选/批量删除确认内容）
4. 整屏详情（§4）+ 打开/关闭/数据渲染测试；删除 describe 弹窗对文件的旧路径
5. 命令补全与历史（§5）+ 单元测试
6. Fsck 简化（§6）
7. Dashboard 行选择（§7）
8. `cargo clippy --all-targets` 零警告；`cargo test` 全绿（65+ 更新 + 新增）
9. README.md 的 Keybindings 表同步更新

## 9. 验收标准

- 所有现有功能仍可达（单测覆盖）
- 每个面板按 `x` 出现菜单且每项可执行
- Files 里 Space 标记 ≥2 个文件后 `x`→Del 出现"N 个标记项"确认
- `d` 打开整屏详情，`Esc` 返回
- `:` 输入 `ls` 时出现补全行，`Tab` 完成，`↑` 能翻历史
- clippy 零警告、全部测试绿
