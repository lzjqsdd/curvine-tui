# curvine-tui

A terminal UI for [Curvine](https://github.com/CurvineIO/curvine), covering the
surface area of `curvine-cli` with live, navigable panels instead of one-shot
commands.

Built on [ratatui](https://github.com/ratatui/ratatui) + crossterm, with the
data layer wired directly into curvine's own crates (`curvine-unified-fs`,
`curvine-job-client`) so behaviour stays aligned with the CLI.

Dependencies are pinned to the upstream tag `v0.6.0-alpha`, so a plain clone
builds after `cargo build`. See [Version coupling](#version-coupling) — the
client and server must be the same version.

## Status

| Panel | Backing CLI commands | State |
|---|---|---|
| Dashboard | `report`, `dfs df` | ✅ single cluster panel (capacity, namespace, members) + worker table |
| Files | `fs ls`, `mkdir`, `rm`, `mv` | ✅ browse, filter, icons, multi-select, batch delete/free, full-screen detail |
| Nodes | `node` | ✅ per-worker endpoint, disk occupancy, full-screen detail |
| Mounts | `mount`, `umount` | ✅ mount form, unmount confirm, mode/storage badges |
| Jobs | `load-list`, `load-status`, `transfer` | ⚠️ jobs started by this session only; cancel supported |
| Transfers | `fs put`, `fs get` | ✅ live progress bars, per-file speed, cancel |
| Fsck | `fsck` | ✅ recursive replica scan, selectable unhealthy files, detail drill-down |
| Bench | `bench` | ✅ metadata + throughput metrics, p50/p95/p99, tunable |
| Command line | all of the above | ✅ `:` prompt, completion and history |

Not yet wired: `cat` viewer, `export`, `transfer` submission, `chmod`/`chown`,
`du`/`count`, `bench prefill`, `mount --check-path-consist` and resync. resync
lives in `curvine-cli` and walks the UFS tree, so a first fs-mode mount made from
the TUI shows the mount but not pre-existing UFS files until `cv mount resync`
runs; mount the path with `cv mount` when that import matters.

### Mounts

`n` opens a form covering write mode (cache/fs), storage class, TTL, auto-cache
(on by default, matching `cv mount`) and read-verify; unmounting requires an explicit confirmation because cached
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

The UI is contextual: the header is the panel menu — each tab carries its
number, icon and short name, the active tab is highlighted, and the cluster
state (master, worker count, refresh countdown) stays pinned on the right. Tabs
degrade from `n icon name` to `n icon` to `n` as the terminal narrows, before
the cluster pill gives up a cell. The footer shows the keys for the current
context, and `x` opens a menu with the same actions so nothing is hidden behind
memorisation.

### Global

| Key | Action |
|---|---|
| `x` | open the contextual action menu |
| `1`–`8` / `tab` / `shift-tab` | switch panel |
| `j` / `k`, `↑` / `↓` | move selection |
| `d` | full-screen detail (files, workers) |
| `r` | refresh the active panel |
| `:` | command line — `tab` completes, `↑`/`↓` browse history, `⏎` runs |
| `?` | help |
| `esc` | close the menu/detail, or clear file marks |
| `q` / `ctrl-c` | quit (detail/menu close instead) |

Any key — or a mouse event — dismisses an error toast, and the key still
performs its normal action: `r` clears the alert *and* refreshes the panel.

### Files

| Key | Action |
|---|---|
| `⏎` / `l` | enter directory |
| `⌫` / `h` | parent directory |
| `/` / `.` | filter listing / toggle hidden files |
| `space` | mark or unmark the highlighted entry |
| `a` / `u` | mark every visible entry / clear all marks |
| `d` | full-screen file detail (blocks + worker distribution) |
| `Del` / `D` | delete / delete recursively — both ask first |
| `F` | free cached blocks |
| `m` | create directory |
| `p` / `g` | upload / download (put / get form) |

With entries marked, the menu offers batch delete/free; the confirmation states
the number of marked items. A row can be both selected and marked: the cursor
and the mark are separate columns (`▶●`, ` ●`, `▶ `), so marking a row never
hides where the cursor is.

### Per panel

| Panel | Keys |
|---|---|
| Dashboard | `↑↓` select worker · `d` worker detail · `r` refresh |
| Nodes | `↑↓` select worker · `d` worker detail · `r` refresh |
| Mounts | `↑↓` select · `n` new mount · `u` unmount (confirm) · `r` refresh |
| Jobs | `↑↓` select · `c` cancel job · `r` refresh |
| Transfers | `↑↓` select · `Del` cancel · `c` clear finished · `p`/`g` upload/download |
| Fsck | `↑↓` select bad file · `⏎` open detail · `s` scan · `f` target browsed dir |
| Bench | `b` run · `p` profile · `t` threads · `d` duration |

In a full-screen detail view, `esc`/`q` returns and `1`–`8`/`tab` switches panel.
The command table is `ls cd mkdir rm mv put get mount umount load fsck bench
panel refresh help quit` (plus `cancel`); the three best prefix matches appear
above the footer while typing, and `tab` completes to the first one.

## Architecture

```
src/
├── main.rs        terminal setup, event loop
├── app.rs         state + reducer (keys, menus, details, commands)
├── config.rs      ClusterConf loading with CLI overrides
├── event.rs       input thread, tick thread, async task results
├── anim.rs        springs, transitions, frame-rate governor
├── transfer.rs    streaming upload/download with throttled progress
├── ui/
│   ├── mod.rs     frame layout, header/footer, shared formatters
│   ├── theme.rs   palette + usage thresholds
│   ├── components.rs  tags, bars, KV lines, inputs, dialogs, toasts, help
│   ├── menu.rs        contextual action menu widget
│   ├── detail.rs      full-screen file/worker detail views
│   ├── mounts.rs      mount table + mount form + unmount confirm
│   ├── transfers.rs   progress table + put/get form
│   ├── fsck.rs        health gauges + unhealthy files
│   ├── bench.rs       knobs + metrics table
│   └── {dashboard,files,nodes,jobs}.rs
└── tests.rs       render smoke tests across panels, states and sizes
```

Long-running RPCs are spawned onto curvine's `Runtime` and report back through a
single `AppEvent` channel, so the UI never blocks on network I/O. Stale
responses (e.g. an `ls` that returns after the user navigated away) are dropped.

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
├── anim.rs        springs, transitions, frame-rate governor
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

## Motion

Motion is used only where it explains a change, never for decoration:

| What animates | Why | Implementation |
|---|---|---|
| capacity and health gauges | a 5 s refresh that snaps sideways is read as a glitch; a bar that travels is read as a change | critically-damped spring |
| namespace counters | files/dirs/blocks count up so the eye can see *how much* changed | spring, integer tolerance |
| transfer progress | progress arrives in bursts; easing makes it monotonic-looking | spring per transfer |
| success/info status | a transient confirmation stays readable even if you look away | wall-clock hold then colour fade in the footer badge |
| error toast | an error must be noticed without blocking the UI, and a repeating refresh error must not flicker | frame-driven hold then colour fade; repeats bump a `×N` counter |

Dialogs deliberately do **not** animate. A modal sits on the critical path to a
decision, and an early attempt at a 150 ms pop-in showed the box at 88% scale
with its footer clipped on the first frame — it read as lag, not as polish.
Dialogs now render complete in a single frame.

Three rules the implementation follows:

- **Springs, not timers.** Anything tracking live data uses a spring, which is
  interruptible: a new value retargets the same spring mid-flight instead of
  restarting a tween, so frequent refreshes bend the curve rather than stutter.
- **First value never animates.** A readout appearing for the first time shows
  its value; only *changes* travel. Otherwise every panel entry would count up
  from zero.
- **Animations must terminate.** Settle thresholds are per-spring (half a unit
  for integer counters, half a thousandth for ratios) with a 1.5 s watchdog, so a
  mis-tuned value cannot pin the UI at 60 fps.
- **Nothing animates on the input path.** Selection, dialogs and forms appear
  instantly; only values that change without the user acting are eased.

The frame rate is governed, not constant: 16 ms while something is moving, 80 ms
while a spinner or transfer is live, 250 ms when idle. The input thread already
polls with a timeout, so it doubles as the frame clock — no extra thread, and an
idle UI measured at 0.0% CPU / ~1.3 MB RSS.

Disable it all with `--no-anim` or `CURVINE_TUI_NO_ANIM=1`.

## Headless mode

The same code paths are reachable without a terminal, which is also how the
integration tests validate them:

```bash
curvine-tui --probe                                   # connectivity + cluster dump
CURVINE_TUI_PROBE_MOUNT='ufs|cv' curvine-tui --probe   # mount
CURVINE_TUI_PROBE_UMOUNT='/cv' curvine-tui --probe    # unmount
curvine-tui --put /local/file.bin /remote/file.bin   # streaming upload
curvine-tui --get /remote/file.bin /local/file.bin   # streaming download
```

`--probe` exits non-zero if any check fails, so it works as a health check.

## Testing

```bash
cargo test                  # hermetic: render, reducer and helper tests
cargo clippy --all-targets  # must stay warning-free
```

The live tests are skipped unless `CURVINE_TUI_LIVE=1`, and they seed their own
fixtures, so they can run against any disposable cluster:

```bash
# 1. build and start a local cluster (one master, one worker)
(cd /path/to/curvine && ./build/build.sh -p core -d && ./build/dist/bin/local-cluster.sh start)

# 2. run everything, serialized so tests do not share fixtures
CURVINE_TUI_LIVE=1 cargo test -- --test-threads=1
```

They expect the cluster at `localhost:8995` and `cv` on `PATH` for the two
fixtures that need a CLI-side mount resync / cache fill (`CURVINE_TUI_CV`
overrides the binary); without `cv` those two tests skip themselves. Everything
else is created through the client and removed afterwards.

The live suite drives the real reducer, not just the RPC layer: header and
cluster context, browsing, multi-select and batch delete, the `x` menu on every
panel, file and worker detail, command completion and history, mount → cache →
free → unmount, `load` jobs, fsck and bench.

## Version coupling

The TUI links curvine's protobuf bindings, so **the client and the cluster must
be the same version**. A newer server sends fields the older bindings cannot
decode; the failure looks like:

```
cluster  : FAILED invalid wire type ... WorkerInfoProto.worker_session_id
```

The Fsck panel reports this honestly rather than showing a green 100% — a
non-empty file with no readable block locations is counted as an error.

To build against a local curvine checkout instead of the pinned tag, uncomment
the `[patch."https://github.com/CurvineIO/curvine"]` block at the end of
`Cargo.toml`.

### Feature parity with upstream

The panel set follows what `curvine-cli` at the pinned tag can actually do:

| Command | Where it lives |
|---|---|
| `report` | Dashboard |
| `fs ls/mkdir/rm/mv/put/get` | Files, Transfers |
| `node` | Nodes |
| `mount`, `umount` | Mounts |
| `load`, `load-status`, `cancel-load` | Jobs |
| `fsck` | Fsck |
| `bench` | Bench |

`admin config` is deliberately absent: upstream has no `admin` command and no
runtime-config RPC, so there was nothing to back a Config panel with.

Not implemented yet: `snapshot`, `export`, and the `transfer` service commands
(`list`, `status`, `tasks`, `cancel`, `retry`, `tenants`). The Jobs panel can
only show jobs started in the current session, because the client library
exposes no "list load jobs" call — only `get_job_status(id)`.

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
| **Badge** | a one-line, text-height label: `◖ text ◗`. The half-circle caps are glyphs, not borders: foreground is the page colour and background is the tone, so the fill ends in rounded caps. The middle is ` text ` in dark ink on the same tone, bold. Used for the cluster state in the header, the footer status, panel strips, the Nodes live/block counters, dialog keycaps, the help panel count and badged panel titles. |
| **Toast** | bottom-centred rounded alert for errors: three rows tall, capped at 60% of the screen, with a `● message` line in the error tone. It floats over panel content with the default background (no solid fill), holds for 4 s, then fades. A repeat of the same error becomes `● message ×2` instead of restarting the banner. Any key dismisses it without swallowing the key's action. |
| **Tag** | inline categorical text (mount mode, storage class, direction) — coloured text only, never a filled block. Table cells are text-only because a cell cannot host a badge's caps. |
| **Bar** | eighth-block progress (`▏▎▍▌▋▊▉`) so short bars keep sub-cell precision; `bar_with_pct` adds a right-aligned percentage. |
| **Bar block** | full-width capacity/health/progress meter in a bordered block, drawn with block characters and a centred label on plain background (no inverted/filled label). |
| **KV line** | `label  value` with a muted label so values carry the weight. |
| **Input / Choice / Toggle** | the three form control styles used by the mount and transfer forms. |
| **Badged panel title** | `unhealthy files — 2 ◖ action needed ◗` — the badge lives inside the border title, which is a single line of text. |
| **Rounded borders** | every panel and dialog uses `BorderType::Rounded`. |
| **Nerd Font icons** | per-file-extension glyphs in the listing, per-panel glyphs in the header menu, and the selected item's glyph in the action menu title. Purely decorative: nothing depends on them semantically. |
| **Value formatting** | owner columns never render as a bare separator: an empty owner is `-`, and the paired form is `owner:group` with `-` for a missing half. The Nodes version column shows `release (7-char commit)` instead of the worker's full build string. |

Every panel also reserves a bottom strip for selection detail or queue counters,
so the same shape repeats across Files, Mounts, Transfers and Fsck. Header,
footer and the selection strips are single-row bands: a badge occupies exactly
its text height, so no band reserves rows for borders.

The only filled backgrounds in the UI are badge interiors, the full-row
selection highlight, and the active tab in the header menu (a subtle underlay,
not a block). A badge's background is exactly as wide as `◖ text ◗` and stops
at the caps, so it can never grow into the solid rectangle this style exists to
remove. The toast clears the cells it covers so text cannot bleed through, but
paints no fill: its body keeps the terminal's default background.
Status dots remain round by design, and `Tag` has no filled variant at all.

The header is two groups: the panel menu on the left and the cluster badge on
the right, separated by at least one cell. The active tab is accent-bold on a
subtle fill; inactive tabs are muted. The menu claims only the width left over
after the pill is placed and degrades `n icon name` → `n icon` → `n`, so the
cluster health badge always keeps its place. The brand lives in the help popup,
not the header. When a panel's key hints do not fit, the footer falls back to a
short generic line — `tab/1-8 panels · : cmd · ? help · q quit` — so switching
panels is never undiscoverable. Empty tables are not left blank — Files
says `empty directory` / `no matches`, Jobs says when session jobs will appear.
Success and info messages stay in the footer badge and are capped at 45% of the
footer width, so a long message can never push the key hints off-screen. Errors
never enter the footer: they are promoted to the toast, which is capped at 60%
of the screen width.

### Two things the UI deliberately does not claim

- **`disk used` is not load.** A worker reports no CPU or IO metric, only
  capacity/available/used bytes. The bar is labelled as occupancy.
- **`replicas` in the file listing is the replication factor**, not the number of
  live copies. It is drawn with a neutral dot and labelled "configured factor" in
  the full-screen detail view. Whether those copies exist is answered by Fsck, which
  compares block locations against the expected count.

Worker identity is always the `ip:port` endpoint. `WorkerAddress`'s `Display`
impl renders `worker_id = 4, addr = host/ip:port`, which is never shown.
