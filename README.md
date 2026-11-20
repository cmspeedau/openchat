# openchat

openchat is a standalone AI chat client built with Rust, egui, and eframe. The planned interface uses an OpenChamber-style three-pane layout and Tokyo Night colors. The project is designed to work with external interfaces only; it does not bundle or host model runtimes.

Planned integrations include OpenCode (including its skills), configurable CPU governor controls, Slop Law (anti-slop prompt and linting tools), and a Telegram bot. Releases are currently planned to be unsigned.

This repository is at Phase 0: a workspace and initial crate scaffolding. Most modules are intentionally stubs; no integrations are implemented yet.

## Workspace

- `app`: eframe desktop application shell
- `core`: configuration and tracing setup
- `providers`: provider integrations (planned)
- `opencode`: OpenCode integration (planned)
- `governor`: CPU governor integration (planned)
- `slop`: Slop Law tools (planned)
- `telegram`: Telegram bot integration (planned)
- `skills`: skill frontmatter parsing and discovery

## Build

Install the stable Rust toolchain, then run `cargo run -p openchat-app`. On Linux, eframe may require the system development libraries documented by its platform dependencies (X11/Wayland and OpenGL/EGL).

## License

The workspace currently declares dual MIT/Apache-2.0 licensing.
