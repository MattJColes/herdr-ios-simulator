# herdr-ios-simulator

An iOS Simulator that a user and their coding agents share inside herdr: the user sees and
touches its screen, agents drive it and read what happened on it. The words match
herdr-android-emulator's glossary wherever the two platforms mean the same thing.

## Language

**Device**:
One iOS simulator, identified by its UDID (for example "iPhone 18 Pro, iOS 27.2"). Booting it
makes it usable; agents' commands always act on a Device.
_Avoid_: simulator instance, emulator, phone

**Current Device**:
The one Device every agent and the View act on when a command names none. Shared, not per
agent; `boot` sets it.
_Avoid_: default device, active device

**Current App**:
The app the plugin last launched, by bundle id. `logs` and `terminate` default to it. An app
launched from Xcode never becomes the Current App.
_Avoid_: target app, active app

**View**:
A herdr pane showing a Device's live screen and taking the user's taps and keys. Closing it
stops the video, not the Device.
_Avoid_: simulator pane, window, mirror

**Element**:
One labelled, valued or identified item on screen, numbered by the last `ui`. Its number means
nothing after the screen changes.
_Avoid_: node, control, widget

**Event**:
One entry on a Device's timeline: boot, shutdown, install, launch, terminate, deep link, build,
tap, mark, or a tap or swipe the user made in the View. Only actions taken through this plugin
become Events.
_Avoid_: log line, activity

**Mark**:
A named point in time on a Device's timeline. `--since <mark>` reads from there.
_Avoid_: checkpoint, bookmark

**Log Entry**:
One message from the Device's unified log (os_log, Logger, NSLog). The app writes Log Entries;
the plugin writes Events.
_Avoid_: event, logcat

**App Output**:
What the app wrote to stdout and stderr since the plugin last launched it.
_Avoid_: console, logs

**Crash**:
An `.ips` crash report written by a process on the Device.
_Avoid_: tombstone, exception
