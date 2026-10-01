# nuthatch-desk

A read-only desktop client for a running [Nuthatch](https://github.com/nightswatchhq/nuthatch)
nest. Rust underneath, Qt 6 and QML on top, joined by [cxx-qt](https://github.com/KDAB/cxx-qt).

It is the workstation counterpart to
[nuthatch-tui-client](https://github.com/nightswatchhq/nuthatch-tui-client), and keeps that
client's contract: it speaks to the nest's HTTP API and nothing else. No store access, no RPC key,
no writes. It adds the three things a terminal cannot hold: a SQL workbench with a real grid,
charts with history, and several nests open at once.

It is also a worked example of QObjects written in Rust, with the ownership and threading rules
written down and tested. The design, the rules and what the build learned are in
[docs/rfc-0001.md](docs/rfc-0001.md).

A hobby project. The name is a placeholder.

![The overview of a nest following the tip](docs/overview.png)

![A table's newest rows](docs/tables.png)

## What it shows

Per nest, in a tab of its own:

- **Overview.** The nest's state in the terminal client's words (live, attention, backfill,
  quarantined, stale), its heights, lag and seal gap, a sync gauge, and six charts over the last
  hour: lag, rows decoded, RPC requests, CPU, memory and the client's own round trip.
- **Tables.** The catalogue by contract, and the newest fifty rows of the selected table, which
  move on in place as blocks arrive.
- **SQL.** A statement against the nest's `/sql`, its result in a grid that sorts by any column,
  and the result copied out as CSV. A refused statement is shown in the nest's own words.

Amounts are shown in base units and compared exactly: a `uint256` sorts as the number it is, not as
text and not as a double.

## Build

`cargo build` is the whole build. There is no CMake. You need a Rust toolchain, a C++ compiler, and
Qt 6 with its QML modules. It is written for Qt 6.5 and newer, and CI builds and tests it against
6.8.3, 6.10.2 and 6.11.2. Nothing older has been tried.

| Platform | Qt |
| --- | --- |
| macOS | `brew install qt` |
| Windows, MSVC | Qt 6.8 or newer for `msvc2022_64`, from Qt's installer, with its `bin` on the path |
| Ubuntu 26.04 | `sudo apt install qt6-base-dev qt6-base-dev-tools qt6-declarative-dev qt6-declarative-dev-tools libgl1-mesa-dev lld qml6-module-qtquick qml6-module-qtquick-controls qml6-module-qtquick-layouts qml6-module-qtquick-shapes qml6-module-qtquick-templates qml6-module-qtquick-window qml6-module-qtqml qml6-module-qtqml-models qml6-module-qtqml-workerscript` |

Ubuntu 24.04's own Qt is 6.4, which is too old.

Qt is found through `qmake` on the path. With several Qt versions installed, or where the binary is
called `qmake6`, set `QMAKE` to the one you mean.

```sh
cargo build --release
./target/release/nuthatch-desk
```

There are no binaries to download. Qt is used under the LGPL and linked dynamically, and shipping
binaries brings obligations this project has not taken on.

## Run

```sh
nuthatch-desk                              # the nests in nests.toml, or http://127.0.0.1:8288
nuthatch-desk --url http://127.0.0.1:8288  # one more tab, beside those
```

It reads the terminal client's file, `~/.config/nuthatch-tui/nests.toml`, so one config serves
both:

```toml
[local]
url = "http://127.0.0.1:8288"

[prod]
url = "http://127.0.0.1:8288"
ssh = "root@nest-host"
```

It opens no ssh forward of its own. A nest the file reaches through ssh gets a tab that names the
host and waits: open a forward with `ssh -N -L 8288:127.0.0.1:8288 root@nest-host`, then press
the tab's connect button. It waits because, without the forward, that URL is a port on your own
machine, and whatever else listens there would be shown under the nest's name. A URL can also be typed into the field at the top right for the session.

Only `http` and `https` URLs are polled. Plain HTTP to anything but this machine is polled with a
warning on the tab, since what it shows can be read and altered on the way.

## Layout

| Crate | Holds | Links Qt |
| --- | --- | --- |
| `nest-client` | The HTTP client, the poller, `nests.toml` | No |
| `desk-core` | The state behind each QObject, and the poller threads | No |
| `desk-bridge` | The five QObjects, through cxx-qt | Yes |
| `desk` | `main.rs`, the QML, the headless tests | Yes |
| `mock-nest` | A stand-in nest for tests, serving bodies recorded from a real one | No |

The logic lives where it can be tested without Qt. The bridge is deliberately thin: each QObject
holds a `desk-core` struct and copies what it says into properties and model signals.

## Test

```sh
cargo test --workspace     # everything; needs Qt
cargo test -p nest-client -p desk-core -p mock-nest   # the half that needs none
tools/lint-qml.sh          # qmllint, after a build
```

`crates/desk/tests/live.rs` runs the real QObjects headless against the mock nest: it loads QML,
drives the objects, and checks what they show. It also destroys a `NestStatus` while its poll is in
flight, and makes a queue call onto a destroyed object on purpose to see it refused.

To see the window without a display, the same test binary will render it to a file:

```sh
DESK_SHOT_URL=http://127.0.0.1:8288 DESK_SHOT_OUT=/tmp/desk.png QT_QPA_PLATFORM=offscreen \
  cargo test -p desk --test live -- --scenario screenshot
```

## Limits

- Read-only, by design and for good.
- Pointed at a runtime's root rather than a nest, it says what the runtime mounts and asks to be
  pointed at one of those. It does not open them itself.
- `decimals` in `nests.toml` is read and not yet applied: amounts are in base units.
- Cancelling a statement stops the client waiting. The nest still finishes it.
- A free-form result's columns come in the order the nest sends them, which is alphabetical.
- On Windows it builds and passes its tests in CI. Nobody has yet sat in front of it there.
- The tests drive the objects and read what they show. None of them clicks.

## Licence

MIT OR Apache-2.0.
