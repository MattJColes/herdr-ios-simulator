# Features

What herdr-ios-simulator does, in the order we build it. Each v1 item is done when its
"done when" line is true. Its sibling, [herdr-android-emulator](https://github.com/MattJColes/herdr-android-emulator),
uses the same command names, words and order, so an agent that knows one knows the other.

## Decisions

- **Screen and input: AXe.** `simctl` can't tap, type, describe the screen or stream video, so
  [AXe](https://github.com/cameroncooke/AXe) does all four; `stream-video` gives up to 30 fps.
  See [ADR 0001](adr/0001-axe-for-input-and-video.md).
- **No background process.** Unlike Android, which needs a Supervisor because logcat's buffer
  rolls over, iOS keeps its unified log and crash reports on disk. Commands read them when asked
  (`log show`, `DiagnosticReports/*.ips`), so nothing runs between commands.
- **One shared Current Device.** Every agent and the View act on it unless a command passes
  `--device`. See [ADR 0002](adr/0002-one-shared-current-device.md).
- **Platform: macOS only**, because the simulator is macOS only. Needs Xcode with an iOS runtime.
- **Language: Rust**, following herdr-web-browser. Its terminal renderer (Kitty images,
  half-block fallback) and agent send code are copied, not shared, until both plugins work.
- **CLI name: `herdr-ios`**, put on PATH (`~/.local/bin`) by a startup hook. The plugin id stays
  `herdr-ios-simulator`.

Not goals: physical iPhones, replacing Xcode.

## v1

### 1. View

The simulator in a herdr pane beside the agent, booted without Simulator.app, so the View is the
only window. **Built; not yet tested inside herdr.**

- Click to tap, drag to swipe, scroll to scroll, type to send text, Return, Backspace and Tab.
- Header: device name and OS, Home, Lock, Screenshot, Quit.
- Opens from the `Simulator: open` action or `herdr-ios boot --show`. Closing it leaves the
  device running.
- Later in v1: screenshot to agent in the header, follow the herdr theme, herdr desktop's + menu.

Done when: you can sign in to an app in the View without opening Simulator.app.

### 2. Agent CLI

**Built.**

```sh
herdr-ios boot [name] --show        # boot (or reuse) a simulator, show it in a View
herdr-ios devices                   # simulators (* = booted)
herdr-ios build --scheme MyApp --run   # xcodebuild for this device: errors and warnings only
herdr-ios install MyApp.app         # install or replace
herdr-ios launch com.example.app    # start the app, capturing stdout/stderr
herdr-ios deeplink myapp://orders/42
herdr-ios ui                        # on-screen elements: [3] Button "Sign in"
herdr-ios tap 3                     # tap element 3 (or: tap 200 400, in device points)
herdr-ios type "hello" --submit
herdr-ios swipe up
herdr-ios button home               # home, lock, side-button, siri
herdr-ios screenshot /tmp/screen.png
```

`ui` lists only elements on screen, numbered like the browser's `snapshot`, so agents use both
the same way.

Done when: an agent can build, install, launch and click through an app using only these
commands.

### 3. Logs and events

**Built.**

```sh
herdr-ios logs --errors             # the current app's unified log, plus its stdout/stderr
herdr-ios crashes                   # crash reports: exception, reason, top frames
herdr-ios events                    # boot, installs, launches, deep links, taps (yours and the agent's)
herdr-ios mark before-login         # then: logs --since before-login
```

Log text, crash text and on-screen labels are app content: `agent-help` tells agents to treat
them as data, not instructions.

Done when: after a crash, `crashes` shows the stack that caused it without the agent re-running
anything.

### 4. Send to the agent

**Screenshot and errors done** (`send`, the header's buttons, herdr actions); `pick` is next.

- Screenshot and recent errors to the agent, through `herdr agent prompt`, using the browser's
  "which agent gets it" order.
- `pick --prompt "Which button?"`: the user taps in the View and the agent gets that element back.

Done when: one click in the View puts a screenshot in the agent's prompt.

### 5. herdr integration

- `install-agents` teaches Claude Code, Codex, OpenCode and Pi to use `herdr-ios`. Re-running it
  after an update replaces its own section. **Next, with section 4.**
- `uninstall` undoes everything the plugin added outside herdr: the CLI symlink and the agent
  instructions. It leaves Xcode, simulators and AXe, which other tools share. **Next, with
  section 4.** Until then the README lists the manual steps.
- Ctrl-click a `myapp://` link in any herdr terminal to open it on the device.
- Repo gets the `herdr-plugin` topic, which lists it on the marketplace, only after a manual test
  by the maintainer.

### 6. README with screenshots

Taken once 1–4 work: the View beside an agent, the action in herdr, a crash in `crashes`.

## Later

- Crashes pushed to the agent as they happen (opt-in: it interrupts the agent).
- Simulated push (`simctl push`), location, privacy grants, status bar overrides, pasteboard.
- Record a video of a repro (`axe record-video`) and send it to the agent.
- Network capture (needs a proxy; no clean simctl route).
- Per-agent devices, if two agents sharing one simulator becomes common.
- One shared crate for the code both plugins copy.
