# curvine-tui

A terminal UI for [Curvine](https://github.com/CurvineIO/curvine), covering the
surface area of `curvine-cli` with live, navigable panels instead of one-shot
commands.

Built on [ratatui](https://github.com/ratatui/ratatui) + crossterm, with the
data layer wired directly into curvine's own crates (`curvine-unified-fs`,
`curvine-job-client`) so behaviour stays aligned with the CLI.

## Status

| Panel | Backing CLI commands | State |
|---|---|---|
| Dashboard | `report`, `dfs df` | ✅ capacity gauge, inode/block counts, live worker table |
| Files | `fs ls`, `mkdir`, `rm`, `mv` | ✅ browse, filter, mkdir, delete |
| Nodes | `node` | ✅ per-worker capacity, usage %, blocks |
| Mounts | `mount`, `umount` | ✅ read-only table (mount/umount pending) |
| Jobs | `load-list`, `load-status`, `transfer` | ✅ list + progress (cancel pending) |
| Config | `admin config` | ✅ runtime config viewer |
| Command line | all of the above | ✅ `:` prompt with CLI-style syntax |

Not yet wired: `fsck`, `bench`, `put`/`get`/`cat` transfers, `export`,
`transfer` submission, `mount`/`umount` mutations, `chmod`/`chown`, `du`/`count`.

## Usage

```bash
cargo build --release
./target/release/curvine-tui --master-addrs 127.0.0.1:8995
```

Options mirror the CLI's precedence rules (`--conf` > `CURVINE_CONF_FILE` > defaults):

```
--conf <PATH>              Configuration file path
--master-addrs <LIST>      Master address list, e.g. 'm1:8995,m2:8995'
--refresh <SECS>           Auto-refresh interval [default: 5]
```

## Keybindings

| Key | Action |
|---|---|
| `tab` / `shift-tab` | switch panel |
| `1`–`6` | jump to panel |
| `j` / `k`, `↑` / `↓` | move selection |
| `enter` / `l` | enter directory |
| `backspace` / `h` | parent directory |
| `/` | filter listing |
| `.` | toggle hidden files |
| `m` | create directory |
| `d` / `D` | delete / delete recursive |
| `r` | refresh panel |
| `:` | command line (`ls`, `cd`, `mkdir`, `rm`, `mv`, `load`, `panel`, `refresh`) |
| `?` | help |
| `q` / `ctrl-c` | quit |

## Architecture

```
src/
├── main.rs        terminal setup, event loop
├── app.rs         state + reducer (all key handling lives here)
├── config.rs      ClusterConf loading with CLI overrides
├── event.rs       input thread, tick thread, async task results
├── rpc/           async facade over UnifiedFileSystem + JobMasterClient
├── ui/
│   ├── mod.rs     frame layout, shared formatters
│   ├── theme.rs   palette
│   ├── components.rs  block/gauge/spinner/help widgets
│   └── {dashboard,files,nodes,jobs}.rs
└── tests.rs       render smoke tests across panels and terminal sizes
```

Long-running RPCs are spawned onto curvine's `Runtime` and report back through a
single `AppEvent` channel, so the UI never blocks on network I/O. Stale
responses (e.g. an `ls` that returns after the user navigated away) are dropped.

## Development

Requires `protoc` (for curvine's proto codegen) and the pinned toolchain in
`rust-toolchain.toml`.

```bash
cargo test      # render smoke tests + pure helper tests
cargo clippy --all-targets
```
