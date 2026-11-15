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
| Dashboard | `report`, `dfs df` | ✅ single cluster panel (capacity, namespace, members) + worker table |
| Files | `fs ls`, `mkdir`, `rm`, `mv` | ✅ browse, filter, icons, replica factor, describe, confirmed delete |
| Nodes | `node` | ✅ per-worker endpoint, disk occupancy, describe |
| Mounts | `mount`, `umount` | ✅ mount form, unmount confirm, mode/storage badges |
| Jobs | `load-list`, `load-status`, `transfer` | ✅ list + progress (cancel pending) |
| Transfers | `fs put`, `fs get` | ✅ live progress bars, per-file speed, cancel |
| Fsck | `fsck` | ✅ recursive replica scan, unhealthy-file drill-down |
| Bench | `bench` | ✅ metadata + throughput metrics, p50/p95/p99, tunable |
| Config | `admin config` | ✅ runtime config viewer |
| Command line | all of the above | ✅ `:` prompt with CLI-style syntax |

Not yet wired: `cat` viewer, `export`, `transfer` submission, `chmod`/`chown`,
`du`/`count`, `bench prefill`, `mount --check-path-consist` and resync.

### Mounts

`n` opens a form covering write mode (cache/fs), storage class, TTL, auto-cache
and read-verify; unmounting requires an explicit confirmation because cached
data on that mount becomes unreachable. Verified against a live cluster:
mounting a local `file://` UFS makes it appear in the table, and unmounting
removes it.

### Fsck

Recursive replica-health scan matching the CLI's semantics exactly: it reports
the master's metadata view (replica state per worker), not on-disk file
presence. Verified field-for-field against `curvine-cli fsck` on a live cluster,
both healthy and with a killed worker. Only unhealthy files are retained in the
UI, since a clean tree would otherwise produce thousands of identical rows.

### Bench

Runs `CurvineBenchRunner` on a background task. Knobs are intentionally a small
subset (`p` profile, `t` threads, `d` duration) so interactive tuning does not
become a flag-parsing exercise.

### Transfers

Uploads and downloads stream in `write_chunk_size` / `read_chunk_size` slices on
curvine's runtime, so the UI never blocks. Progress is throttled to ~8 events per
second regardless of file size or bandwidth, and each transfer carries its own
cancellation flag that the worker checks between chunks.

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
| `1`–`7` | jump to panel |
| `j` / `k`, `↑` / `↓` | move selection |
| `enter` / `l` | enter directory |
| `backspace` / `h` | parent directory |
| `/` | filter listing |
| `.` | toggle hidden files |
| `m` | create directory |
| `d` / `D` | delete / delete recursive |
| `p` / `g` | upload / download (put / get form) |
| `n` / `u` | new mount (form) / unmount selection (confirm) |
| `d` | describe the selected file (blocks) or worker |
| `Del` / `D` | delete / delete recursively — both ask first |
| `F` | free cached blocks for the selection |
| `enter` (Fsck) | scan; `f` targets the browsed directory; `x` cancels |
| `enter` (Bench) | run; `p`/`t`/`d` tune profile/threads/duration |
| `x` | cancel selected transfer |
| `c` | clear finished transfers |
| `r` | refresh panel |
| `:` | command line (`ls`, `cd`, `mkdir`, `rm`, `mv`, `put`, `get`, `cancel`, `load`, `panel`, `refresh`) |
| `?` | help |
| `q` / `ctrl-c` | quit |

## Architecture

```
src/
├── main.rs        terminal setup, event loop
├── app.rs         state + reducer (all key handling lives here)
├── config.rs      ClusterConf loading with CLI overrides
├── event.rs       input thread, tick thread, async task results
├── ui/components.rs  shared widgets (tags, bars, inputs, popups, help)
│   ├── fsck.rs    recursive replica scan
│   └── bench.rs   benchmark harness wrapper
├── transfer.rs    streaming upload/download with throttled progress
├── ui/
│   ├── mod.rs     frame layout, shared formatters
│   ├── theme.rs   palette + usage thresholds
│   ├── components.rs  tags, bars, KV lines, inputs, popups
│   ├── mounts.rs      mount table + mount form + unmount confirm
│   ├── transfers.rs   progress table + put/get form
│   ├── fsck.rs        health verdict + unhealthy files
│   ├── bench.rs       knobs + metrics table
│   └── {dashboard,files,nodes,jobs}.rs
└── tests.rs       render smoke tests across panels and terminal sizes
```

Long-running RPCs are spawned onto curvine's `Runtime` and report back through a
single `AppEvent` channel, so the UI never blocks on network I/O. Stale
responses (e.g. an `ls` that returns after the user navigated away) are dropped.

## Headless mode

The same code paths are reachable without a terminal, which is also how the
integration tests validate them:

```bash
curvine-tui --probe                                  # connectivity + cluster dump
CURVINE_TUI_PROBE_MOUNT='ufs|cv' curvine-tui --probe  # mount
CURVINE_TUI_PROBE_UMOUNT='/cv' curvine-tui --probe    # unmount
curvine-tui --put /local/file.bin /remote/file.bin   # streaming upload
curvine-tui --get /remote/file.bin /local/file.bin   # streaming download
```

`--probe` exits non-zero if any check fails, so it works as a health check.

## Development

Requires `protoc` (for curvine's proto codegen) and the pinned toolchain in
`rust-toolchain.toml`.

```bash
cargo test                 # hermetic: render smoke tests + pure helpers
cargo clippy --all-targets
```

Integration tests against a real cluster are opt-in:

```bash
# start a cluster (see curvine's docs), then:
CURVINE_TUI_LIVE=1 CURVINE_CONF_FILE=/path/to/curvine-cluster.toml \
    cargo test --test live_transfer -- --test-threads=1
```

They cover probe health, byte-identical upload/download round-trips, a clean
failure for a missing remote file, fsck health, benchmark metrics, mount/umount
round-trips, and a clean failure when unmounting something that is not mounted.

## Visual language

Panels share one widget vocabulary so the UI reads consistently:

| Control | Usage |
|---|---|
| **Status dot** | health and state: `● healthy`, `● lost`, `○ n/a`. Colour comes from `tone_for_status`, so a state can never be green in one panel and grey in another. The help popup documents the palette. |
| **Tag** | categorical values only (mount mode, storage class, direction) — boxes are reserved for things that are not health. |
| **Bar** | eighth-block progress (`▏▎▍▌▋▊▉`) so short bars keep sub-cell precision; `bar_with_pct` adds a right-aligned percentage. |
| **Gauge** | full-width capacity/health/progress meter in a bordered block. |
| **KV line** | `label  value` with a muted label so values carry the weight. |
| **Input / Choice / Toggle** | the three form control styles used by the mount and transfer forms. |
| **Badged panel title** | `mounts — 3 entries  [2 stale]`. |
| **Rounded borders** | every panel and dialog uses `BorderType::Rounded`. |
| **Nerd Font icons** | per-file-extension glyphs in the listing, per-panel glyphs in the tabs. Purely decorative: nothing depends on them semantically. |

Every panel also reserves a bottom strip for selection detail or queue counters,
so the same shape repeats across Files, Mounts, Transfers and Fsck.

### Two things the UI deliberately does not claim

- **`disk used` is not load.** A worker reports no CPU or IO metric, only
  capacity/available/used bytes. The bar is labelled as occupancy.
- **`replicas` in the file listing is the replication factor**, not the number of
  live copies. It is drawn with a neutral dot and labelled "configured factor" in
  the describe popup. Whether those copies exist is answered by Fsck, which
  compares block locations against the expected count.

Worker identity is always the `ip:port` endpoint. `WorkerAddress`'s `Display`
impl renders `worker_id = 4, addr = host/ip:port`, which is never shown.
