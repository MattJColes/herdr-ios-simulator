# Glossary

**Device**: one iOS simulator, identified by its UDID (for example "iPhone 18 Pro, iOS 27.2"). Not a physical iPhone.

**Current device**: the one device every agent and the pane act on unless a command passes `--device`. Shared, not per agent. `boot` sets it.

**Current app**: the app the plugin last launched (by bundle id). `logs` and `terminate` default to it. An app launched from Xcode never becomes the current app.

**Pane** (live view): the herdr pane showing the current device. Closing it stops the video, not the device.

**Element**: one labelled, valued or identified item in the device's accessibility tree, numbered by the last `describe`. Its number means nothing after the screen changes.

**Mark**: a named point in time on a device's timeline. `--since <mark>` reads from there.

**Event**: one entry on a device's timeline: boot, shutdown, install, launch, terminate, open URL, build, tap, mark, or a user tap or swipe in the pane. Only actions taken through this plugin become events.

**Log entry**: one message from the device's unified log (os_log, Logger, NSLog). Distinct from an event: the app writes log entries; the plugin writes events.

**App output**: what the app wrote to stdout and stderr since the plugin last launched it.

**Crash**: an `.ips` crash report written by a process on the device.
