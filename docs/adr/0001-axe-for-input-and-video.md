# AXe for input and video

`simctl` can boot, install, launch and read logs, but it can't tap, type, describe the screen or stream video. We depend on AXe (`brew install cameroncooke/axe/axe`) for all four rather than writing our own Swift helper on Apple's private SimulatorKit framework, which would have meant maintaining private-API code. AXe's `stream-video` (up to 30 fps) also replaced the planned `simctl io screenshot` polling (a few fps), since AXe was needed anyway. Neither route needs Screen Recording or Accessibility permission.

## Consequences

Users install AXe themselves, and AXe follows Xcode releases (it moved to Device Hub for Xcode 27). Its first call after a boot can time out while it sets up its automation session; a retry works. If AXe breaks or lags behind Xcode, the fallback is that Swift helper.
