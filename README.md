# MindRune

<img src="assets/icon/mindrune-128.png" alt="The MindRune icon" width="96">

MindRune is a desktop window for [Claude Code](https://claude.com/claude-code) that puts a wide screen to use. Several sessions sit side by side in one window, each as wide as you like, and you can see at a glance which are working, which are waiting for you and which have finished.

It is written in Rust on the [Rune](https://github.com/anomalyco/rune) toolkit. It is an unofficial client: it does not talk to a model itself. It starts the `claude` program you already have, and Claude Code does all the agent work.

## Why

On an ultrawide monitor a single narrow conversation column leaves most of the screen empty. And with several sessions running in different projects, a lot of time goes into switching between them only to check whether one is done or needs an answer. MindRune is built to remove both.

## Goals

- The width of a conversation is your choice, never a fixed narrow column.
- Several sessions are visible and usable at once, tiled side by side.
- The window always shows which sessions are working, which need you and which have finished.
- The model in use is visible wherever you type.
- The usage limits are always visible as plain text.
- Everything else behaves the way a Claude Code user expects.

## What it does today

- Sessions in tiles, side by side, with up to two stacked in a column.
- A sidebar of sessions grouped by folder, read from Claude Code's own session history.
- Permission requests and questions answered in the window.
- Pickers for the model, the effort level and the permission mode.
- A panel for the focused session: its context, its file changes and a Remote Control switch.
- Attachments: images pasted, picked or dropped, and files by path. A press on a picture opens it large.
- The app keeps running when its window is closed, with a tray icon, so sessions carry on in the background.

Screenshots will follow.

## Running it

You need Claude Code installed and signed in, and the Rust toolchain named in `rust-toolchain.toml`. Linux comes first; other systems are not yet looked at.

```bash
cargo run --release -p mindrune
```

Closing the window does not end the app. Quit with Ctrl+Q, from the tray icon's menu, or with:

```bash
cargo run --release -p mindrune -- --quit
```

To look at the window without Claude Code, with made-up sessions:

```bash
cargo run -p mindrune -- --demo
```

On a Linux desktop, `scripts/install-desktop-entry.sh` adds MindRune and its icon to the launcher.

## How it is built

The window never speaks to Claude Code directly. It talks to an agent through a small seam, so the two can change apart:

| Crate | What it is |
| --- | --- |
| `crates/agent` | The seam between a window and an agent, with a fake agent for tests |
| `crates/claude` | The adapter that runs Claude Code and reads its session history |
| `crates/core` | The app's state and rules, with no window |
| `crates/app` | The window, on Rune |
| `crates/probe` | A tool that records the protocol for test fixtures |

The design and the plans it was built from are in `docs/`.

## License

MIT. See [LICENSE](LICENSE).
