# Features

What herdr-ios-simulator needs to do, in the order we build it. Modelled on
[herdr-web-browser](https://github.com/MattJColes/herdr-web-browser): one Rust binary that is
both the herdr pane and the CLI agents call, plus a `herdr-plugin.toml` that wires it into herdr.

## Goal

An iOS Simulator in a herdr pane, beside your coding agent. You or the agent can open it. The
agent can build, install and launch the app, see the screen, tap and type, and read everything
the simulator says: logs, crashes and lifecycle events. You keep using the simulator while the
agent works.

Not goals: real devices, Android, replacing Xcode. macOS only (the simulator is macOS only).

## Requirements on the user's Mac

- Xcode with an iOS runtime installed (`xcrun simctl` must work).
- An input helper for taps, swipes, typing and the accessibility tree, because `simctl` has no
  touch or keyboard commands. See [Open questions](#open-questions).
- No Screen Recording or Accessibility permission. Everything goes through the simulator's own
  tooling, not screen capture of the Simulator.app window.

## 1. Run the simulator in a pane (MVP)

| # | Feature | How |
|---|---|---|
| 1.1 | Pick a device | `simctl list devices -j`. Popup picker (like the web browser's agent picker) when no device is booted; remembers the last one. |
| 1.2 | Boot, shut down, erase | `simctl boot` / `shutdown` / `erase`. Boots headless: Simulator.app never has to open. |
| 1.3 | Live screen in the pane | Frames drawn with Kitty images (herdr desktop, Ghostty, kitty, WezTerm), half blocks elsewhere. Reuse `render.rs` from herdr-web-browser. Frame source: see open questions. |
| 1.4 | Touch | Click = tap, drag = swipe, scroll wheel = scroll. Pane cells map to device points. |
| 1.5 | Keyboard | Typed text goes to the focused field. Hardware buttons from a toolbar: Home, Lock, Siri, volume, rotate. |
| 1.6 | Toolbar | Device name and OS, rotate, home, screenshot to agent, appearance (light/dark), window button (open the real Simulator.app on the same device). |
| 1.7 | Opens from herdr | A `Simulator: open` action. Placement `split`, beside the focused agent. herdr desktop's `+` menu (desktop mods) comes after the TUI pane is proven. |
| 1.8 | Agent can open it | `herdr-ios-simulator open --show` opens the pane beside the calling agent if it isn't open. |

## 2. Logs and events for agents (MVP)

Everything is recorded to a per-device NDJSON file under `$HERDR_PLUGIN_STATE_DIR`, so the agent
reads it on demand instead of having it streamed into its context.

| # | Feature | How |
|---|---|---|
| 2.1 | Unified log | `simctl spawn <udid> log stream --style ndjson`, filtered to the app's process by default. `--all`, `--level`, `--subsystem`, `--grep` to widen or narrow. |
| 2.2 | App stdout/stderr | `simctl launch --console-pty` (or `--stdout`/`--stderr` files) when the plugin launches the app, so `print()` output is captured. |
| 2.3 | Crashes | Watch `~/Library/Logs/DiagnosticReports/*.ips` for reports from this device; summarise the exception and the crashing thread's top frames. |
| 2.4 | Lifecycle events | Boot, shutdown, install, launch, terminate, crash, open URL, push sent, location set, appearance changed, user tap/type in the pane. One timeline. |
| 2.5 | Read commands | `logs`, `crashes`, `events`, each with `--since <time\|marker>`, `--follow`, `--clear` and `--json`. Errors first, like the web browser's `network`. |
| 2.6 | Marker | `herdr-ios-simulator mark "before login"` drops a marker so `logs --since "before login"` returns only what happened after. |
| 2.7 | Prompt-injection label | Log text and on-screen text sent to an agent are labelled as app content, not instructions. |

## 3. Agent control CLI (MVP)

Same shape as `herdr-web-browser`: short verbs, `--json` everywhere, `agent-help` prints the list.

```sh
herdr-ios-simulator devices                       # list devices and runtimes
herdr-ios-simulator boot "iPhone 17 Pro" --show   # boot and show it in a pane
herdr-ios-simulator build --scheme MyApp          # xcodebuild for this device, errors summarised
herdr-ios-simulator install build/MyApp.app
herdr-ios-simulator launch com.example.MyApp      # captures stdout/stderr
herdr-ios-simulator screenshot /tmp/screen.png
herdr-ios-simulator describe                      # accessibility tree: [4] button "Sign in"
herdr-ios-simulator tap 4                         # by element number, or: tap --at 120,480
herdr-ios-simulator type "hello" --submit
herdr-ios-simulator swipe up
herdr-ios-simulator button home
herdr-ios-simulator logs --errors                 # what the app logged
herdr-ios-simulator crashes
herdr-ios-simulator openurl myapp://settings
```

`build` runs `xcodebuild -destination id=<udid>` and returns the compiler errors and warnings
only, with file:line, not the full build log.

## 4. Send to the agent (MVP)

| # | Feature | How |
|---|---|---|
| 4.1 | Screenshot to agent | Toolbar button and action. Saves a PNG and sends its path through `herdr agent prompt`. |
| 4.2 | Recent logs to agent | Sends the last errors and warnings since launch. |
| 4.3 | Pick an element | `pick --prompt "Which button?"`: the user taps in the pane and the agent gets that element back. |
| 4.4 | Which agent | Same rules as herdr-web-browser: the agent the pane was opened beside, then last picked, then `default_agent`, then ask. |

## 5. Herdr integration

| # | Item | Notes |
|---|---|---|
| 5.1 | `herdr-plugin.toml` | `platforms = ["macos"]`, `[[build]] cargo build --release`, `[[startup]] install-cli`. |
| 5.2 | Panes | `simulator` (split), `picker` (popup: device and agent pickers). |
| 5.3 | Actions | open, boot, shutdown, send-screenshot, send-logs, toggle-appearance, install-agents, install-cli. |
| 5.4 | Events | `pane.closed` stops streaming (the device keeps running); `pane.moved` follows the pane. |
| 5.5 | Link handler | Ctrl-click a custom-scheme link (`myapp://…`) in a terminal to open it in the simulator. Schemes come from config. |
| 5.6 | `install-agents` | Teaches Claude Code, Codex, OpenCode and Pi to use this CLI for iOS work, in every project. |
| 5.7 | Marketplace | GitHub topic `herdr-plugin` on the repo; manifest at the root. |

## 6. Later

- Simulated push (`simctl push`), location (`simctl location`), privacy grants, status bar
  overrides, pasteboard sync, add photos.
- Record a video of a repro (`simctl io recordVideo`) and send it to the agent.
- Network requests from the app (needs a proxy; no clean simctl route).
- Several devices side by side; iPad and Apple Watch.
- SwiftUI preview-style hot reload loop: watch files, rebuild, relaunch.

## Open questions

1. **Frame source.** Decided: AXe's `stream-video` (length-prefixed JPEG frames, up to 30 fps),
   since AXe is already needed for input. A Swift helper on SimulatorKit could replace it later.
2. **Input.** Decided: [AXe](https://github.com/cameroncooke/AXe) for taps, typing and
   `describe-ui`. Folding both into one Swift helper would drop the extra install.
3. **Which app's logs by default.** Decided: the current app, the one the plugin last launched.
   `logs` names the app it filtered on, and `--bundle` overrides it.
4. **Order of work.** Decided: hand-test the pane, then section 4 (send to agent), then desktop mods.
