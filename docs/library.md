# As a library

tmux-companion could also used as layer for your own UX in tmux, providing it as library would enable anyone interested to write functionality or features based on existing API or send PR or Issue to request new or fix existing.

The list below might not be comprehensive, please report if you think something is missing here or should be made part of Public API.

```sh
cargo add tmux-companion
```

```rust
use tmux_companion::daemon::Daemon;
use tmux_companion::proto::GstArgs;

let segment = Daemon::new()?.git_status(&GstArgs {
    path: Some("/path/to/repo".into()),
    ..GstArgs::default()
})?;
```

**Official crate docs at [https://docs.rs/tmux-companion](https://docs.rs/tmux-companion).**
