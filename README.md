# WARNING

This form contains a bunch of *awful* hacks to make `smallvil/` a Wayland compositor that attempts to render to a terminal.

## Wait what why

Idk, it sounded funny in my head

## Details

* Ratatui + crossterm is used as the backend renderer.
* All pixels are converted to series of U+2584 LOWER HALF BLOCK characters - each row of characters corresponds to 2 consecutive rows of pixels.
* Display resolution is inferred from the terminal size.

Jank:

* Key release events are messed up. crossterm can only emit them when kitty terminal protocol is used.
* Mouse coordinates can't handle resizing the window after startup.
* IT'S SLOOOOOOOW
* You can't stop it with Ctrl+C, only `kill` it from another terminal

With that said, to try this mess out run:

```
cd smallvil && cargo run --release 2>/dev/null
# shut down from another terminal
killall smallvil
```

Note: works ~best~least bad with [alacritty](https://alacritty.org/) configured to use a font with 2x1 pixel sized characters:

```
# save to tiny-font.toml
[window]
# 1024x768px
dimensions = { columns = 1024, lines = 384 }

[font]
normal = { family = "Nimbus Mono PS" }
size = 1
```

```
alacritty --config-file tiny-font.toml --command bash -c "cd $PWD && cargo run --release 2>/tmp/log"
```

<img align="right" width="25%" src="https://github.com/Smithay/smithay/assets/20758186/7a84ab10-e229-4823-bad8-9c647546407b">

# Smithay

[![Crates.io](https://img.shields.io/crates/v/smithay.svg)](https://crates.io/crates/smithay)
[![docs.rs](https://docs.rs/smithay/badge.svg)](https://docs.rs/smithay)
[![Build Status](https://github.com/Smithay/smithay/workflows/Continuous%20Integration/badge.svg)](https://github.com/Smithay/smithay/actions)
[![Join the chat on matrix at #smithay:matrix.org](https://img.shields.io/badge/%5Bm%5D-%23smithay%3Amatrix.org-blue.svg)](https://matrix.to/#/#smithay:matrix.org)
![Join the chat via bridge on #smithay on libera.chat](https://img.shields.io/badge/IRC-%23Smithay-blue.svg)

A smithy for rusty wayland compositors

## Goals

Smithay aims to provide building blocks to create wayland compositors in Rust. While not
being a full-blown compositor, it'll provide objects and interfaces implementing common
functionalities that pretty much any compositor will need, in a generic fashion.

It supports the [core Wayland protocols](https://gitlab.freedesktop.org/wayland/wayland), the official [protocol extensions](https://gitlab.freedesktop.org/wayland/wayland-protocols), and *some* external extensions, such as those made by and for [wlroots](https://gitlab.freedesktop.org/wlroots/wlr-protocols) and [KDE](https://invent.kde.org/libraries/plasma-wayland-protocols)
<!-- https://github.com/Smithay/smithay/pull/779#discussion_r993640470 https://github.com/Smithay/smithay/issues/778 -->

Also:

* **Documented:** Smithay strives to maintain a clear and detailed documentation of its API and its
  functionalities. Compiled documentations are available on [docs.rs](https://docs.rs/smithay) for released
  versions, and [here](https://smithay.github.io/smithay) for the master branch.
* **Safety:** Smithay will target to be safe to use, because Rust.
* **Modularity:** Smithay is not a framework, and will not be constraining. If there is a
  part you don't want to use, you should not be forced to use it.
* **High-level:** You should be able to not have to worry about gory low-level stuff (but
  Smithay won't stop you if you really want to dive into it).

## Anvil

Smithay as a compositor library has its own sample compositor: anvil.

To get informations about it and how you can run it visit [anvil README](https://github.com/Smithay/smithay/blob/master/anvil/README.md)

## Other compositors that use Smithay

* [Cosmic](https://github.com/pop-os/cosmic-epoch): Next generation Cosmic desktop environment
* [Catacomb](https://github.com/catacombing/catacomb): A Wayland Mobile Compositor
* [MagmaWM](https://github.com/MagmaWM/MagmaWM): A versatile and customizable Wayland Compositor
* [Niri](https://github.com/YaLTeR/niri): A scrollable-tiling Wayland compositor
* [Strata](https://github.com/StrataWM/strata): A cutting-edge, robust and sleek Wayland compositor
* [Pinnacle](https://github.com/Ottatop/pinnacle): A WIP Wayland compositor, inspired by AwesomeWM
* [Sudbury](https://gitlab.freedesktop.org/bwidawsk/sudbury): Compositor designed for ChromeOS
* [wprs](https://github.com/wayland-transpositor/wprs): Like [xpra](https://en.wikipedia.org/wiki/Xpra), but for Wayland, and written in
Rust.
* [Local Desktop](https://github.com/localdesktop/localdesktop): An Android app for running GUI Linux via PRoot and Wayland.

## System Dependencies

(This list can depend on features you enable)

* `libwayland`
* `libxkbcommon`
* `libudev`
* `libinput`
* `libgbm`
* [`libseat`](https://git.sr.ht/~kennylevinsen/seatd)
* `xwayland`

## Contact us

If you have questions or want to discuss the project with us, our main chatroom is on Matrix: [`#smithay:matrix.org`](https://matrix.to/#/#smithay:matrix.org).
