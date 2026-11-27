# Changelog

## [1.1.3](https://github.com/MattJColes/herdr-ios-simulator/compare/v1.1.2...v1.1.3) (2026-11-27)


### Bug Fixes

* **deps:** bump the cargo-minor-and-patch group with 2 updates ([8600384](https://github.com/MattJColes/herdr-ios-simulator/commit/8600384f5953f2fa8106e4c12f4063f0850d3ed9))
* **deps:** bump the cargo-minor-and-patch group with 2 updates ([5f9936e](https://github.com/MattJColes/herdr-ios-simulator/commit/5f9936e4665292bb4fcc590551ddb446e0054d81))

## [1.1.2](https://github.com/MattJColes/herdr-ios-simulator/compare/v1.1.1...v1.1.2) (2026-11-18)


### Bug Fixes

* **deps:** bump resvg from 0.49.0 to 0.50.0 ([8986806](https://github.com/MattJColes/herdr-ios-simulator/commit/898680677939dd4a920d42844abccb75b7d0a427))
* **deps:** bump resvg from 0.49.0 to 0.50.0 ([4bd064f](https://github.com/MattJColes/herdr-ios-simulator/commit/4bd064f2e7c170dc8e11c9fc5b6275cb8b3bb387))
* preserve simulator proportions using terminal cell metrics ([a927fb7](https://github.com/MattJColes/herdr-ios-simulator/commit/a927fb732bcfc42d6b0e34bd5e4a15c83736d9d6))
* preserve simulator proportions using terminal cell metrics ([bff2a0a](https://github.com/MattJColes/herdr-ios-simulator/commit/bff2a0a08e8cf81eb22af6fdebad5a3ec1843444))

## [1.1.1](https://github.com/MattJColes/herdr-ios-simulator/compare/v1.1.0...v1.1.1) (2026-11-11)


### Bug Fixes

* release patch versions for cargo dependency bumps ([167948f](https://github.com/MattJColes/herdr-ios-simulator/commit/167948f23cd49edec52e03ef1e446ef390dff023))
* release patch versions for cargo dependency bumps ([b581e5c](https://github.com/MattJColes/herdr-ios-simulator/commit/b581e5c503dbc1bef4d200597fd6d8625baefcc8))

## [1.1.0](https://github.com/MattJColes/herdr-ios-simulator/compare/v1.0.0...v1.1.0) (2026-11-03)


### Features

* set up CLI, desktop and agents on install ([4f47080](https://github.com/MattJColes/herdr-ios-simulator/commit/4f4708072328c16828dfb2a41b163811269c2e55))
* set up CLI, desktop and agents on install ([9e16930](https://github.com/MattJColes/herdr-ios-simulator/commit/9e1693096d8c90748039051ae72432ec928208b1))

## [1.0.0](https://github.com/MattJColes/herdr-ios-simulator/compare/v0.1.0...v1.0.0) (2026-11-03)


### ⚠ BREAKING CHANGES

* the CLI is renamed from herdr-ios-simulator to herdr-ios, and describe/openurl/tap --at are replaced by ui/deeplink/tap X Y.

### Features

* align commands and words with herdr-android-emulator ([48d9ded](https://github.com/MattJColes/herdr-ios-simulator/commit/48d9ded78993f172296c76a0a20f5b0166ae05b7))
* open from herdr desktop's + menu ([14f48fd](https://github.com/MattJColes/herdr-ios-simulator/commit/14f48fd039e30c594c03dd9a29f47ae62337e22b))
* pick elements for the agent and set up agents to use herdr-ios ([a6a4e4a](https://github.com/MattJColes/herdr-ios-simulator/commit/a6a4e4a13b0371c7203bd76d1f5df22f93d16671))
* pick elements for the agent and set up agents to use herdr-ios ([e49d663](https://github.com/MattJColes/herdr-ios-simulator/commit/e49d6634d1521191fb5e16891539f0a06aded436))
* run end to end in herdr with icon buttons and send to agent ([3e927a7](https://github.com/MattJColes/herdr-ios-simulator/commit/3e927a73ff8577f02a2c904f7899553a5ada80c0))
* run end to end in herdr with icon buttons and send to agent ([27486eb](https://github.com/MattJColes/herdr-ios-simulator/commit/27486ebdc4d319b1192624eea5b741ef9ae982c2))
* scaffold the iOS Simulator plugin for herdr ([5795b30](https://github.com/MattJColes/herdr-ios-simulator/commit/5795b30a37d768a697f89efa2f5730f43eeca274))
* scaffold the iOS Simulator plugin for herdr ([cb03026](https://github.com/MattJColes/herdr-ios-simulator/commit/cb03026ff5b50c58289e238fe13464ca777f4e6c))
* set up herdr desktop at startup and add an iOS Simulator menu ([08fa6c3](https://github.com/MattJColes/herdr-ios-simulator/commit/08fa6c3ed94171121587ff7ac17500f284f23750))
* ship the agent skill as a file any harness can install ([dd2bc15](https://github.com/MattJColes/herdr-ios-simulator/commit/dd2bc1560ac00ab83e93a10c55f0a48a181d2c36))


### Bug Fixes

* agent swipes scroll the same every time and Home always lands ([f936044](https://github.com/MattJColes/herdr-ios-simulator/commit/f936044d1eea02a98e0809b5dcf06d6d14d67bfc))
* agent swipes scroll the same every time and Home always lands ([d457c2a](https://github.com/MattJColes/herdr-ios-simulator/commit/d457c2aeb7ff31ba8a83804e236f4f51d8790598))
* drop the view header's idle usage hint ([1a67794](https://github.com/MattJColes/herdr-ios-simulator/commit/1a6779405bc6e45423cacd8df6e3d642f66d860b))
* find Homebrew's AXe, faster taps, installable skill, desktop menu ([c58ae32](https://github.com/MattJColes/herdr-ios-simulator/commit/c58ae32cf54891f5966ea153ca85deeeeddf64b4))
* keep the pick hint short enough to fit the header ([4b724c4](https://github.com/MattJColes/herdr-ios-simulator/commit/4b724c4ef8300aa697a971b38ba1913e98e20f90))
* use Homebrew's AXe before any other axe on PATH ([177775e](https://github.com/MattJColes/herdr-ios-simulator/commit/177775e4438cf61cf235201b5487da482dbbcd87))
* view taps, drags and scrolling land reliably ([eb0e7ed](https://github.com/MattJColes/herdr-ios-simulator/commit/eb0e7edfd3bd7d15634b620119634bd5444a22ba))


### Performance Improvements

* skip AXe's orientation check on every touch ([3e5515e](https://github.com/MattJColes/herdr-ios-simulator/commit/3e5515e21322a2466c43bcd219f975e802b470f6))
