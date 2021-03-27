# `cargo-wop` - cargo without project

**WARNING:** this package is experimental at the moment.

Rust source files as self-contained projects. `cargo-wop` allows to `cargo` work
with rust source file as if thy were full projects.   This project is heavily
inspired by [cargo-script][cargo-script], [cargo-eval][cargo-eval]. In contrast
to these projects, `cargo-wop` is designed to be as close as possible to cargo
and support all sensible arguments.

Run a file as a script (or the [default command](#how-arguments-are-interpreted)):

```bash
cargo wop my-script.rs

# equivalent call:
cargo wop run my-script.rs
```

Build artifacts defined in the script:

```
cargo wop build my-script.rs
```

Run tests define in the script:

```
cargo wop test my-script.rs
```
## How arguments are interpreted

At the moment the following cargo commands are supported: `bench`, `build`,
`check`, `clean`, `clippy`, `fmt`, `install`, `locate-project`, `metadata`,
`pkgid`, `run`, `tree`, `test`, `verify-project`. For most commands `cargo-wop`
rewrites the command line as follows:

```bash
# Original command-line
cargo wop [cargo-command] [script] [args...]

# Rewritten command line
cargo [cargo-command] --manifest-path [generated_manifest] [args...]
```

Some commands use additional rules:

- `new`: create a new source file based on templates. Run `cargo wop new` to get
  a list of all available templates. Run `cargo wop new template SOURCE.rs` to
  create the file. For example use `cargo wop new --lib SOURCE.rs` to create a
  shared library
- `run`: all arguments are passed per default to the script, not to cargo. To
  pass arguments to `cargo` place them before a `--`. For example: `cargo wop
  run my-script.rs --verbose -- ...`
- `build`: is executed twice. Once to build the package and a second time to
  determine the generated build artifacts and copy them into the local folder
- `build` and `run` default to release builds. To disable this behavior, use the
  `build-debug` and `run-debug` commands
- `install`: no manifest path is added, but the `--path` argument to the
  manifest directory

Custom commands:

- `manifest`: print out the generated manifest
- `write-manifest`: write the manifest into the current working directory

If no command is specified the default command. Without additional configuration
it is set to `run`. It can be configured via

```rust
//! ```cargo
//! [cargo-wop]
//! default-action = [COMMAND, ..ARGS]
//! ```
```

It is interpreted as `COMMAND FILE ..ARGS ..CLI_ARGS`, where `CLI_ARGS` are the
arguments passed via the command line. Without configuration, it corresponds to
`default-action = ["run"]`. For example, to build the given file as a wasm32
library set it to `default-action = ["build", "--target",
"wasm32-unknown--unknown"]`.

## Specifying dependencies

Dependencies are described in a cargo manifest embedded in the top-level
comment. Importantly, the file must start with the comment for the manifest to
be recognized. For example:

```rust
//! My script
//!
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! ```
//!
```

The embedded manifest can contain any keys recognized by cargo. `cargo-wop`
normalizes this manifest and makes sure the source file is correctly included.
It also normalizes any paths used to specify dependencies. To show the generated
manifest use:

```bash
cargo wop manifest my-script.rs
```
For example, simply specify a `[lib]` target with the correct flags set to build
a static C library:

```rust
//! My script
//!
//! ```cargo
//! [lib]
//! crate-type = ["cdylib"]
//!
//! [dependencies]
//! ```
```

This script can be built into a library via:

```bash
cargo wop build my-script.rs
```

# Using cargo wop as a VS Code build command

To setup cargo wop as build command in VS Code, that can be accessed via
"Ctrl-Shift-B", create the file `.vscode/tasks.json` with the following
contents:

```json
{
    "version": "2.0.0",
    "tasks": [
        {
            "label": "cargo wop",
            "type": "process",
            "command": "cargo",
            "args": ["wop", "${file}"],
            "group": "build",
            "options": {"cwd": "${fileDirname}"},
            "problemMatcher": ["$rustc"]
        }
    ]
}
```

The `task.json` is documented [here][task-json]. Now pressing "Ctrl-Shift-B" and
selecting cargo wop will execute the currently opened file using cargo wop.

[task-json]: https://go.microsoft.com/fwlink/?LinkId=733558

# Development tasks

Common tasks are bundled in the `make.rs` script. It can be used with
`cargo-wop` itself. First install `cargo-wop`. Then run the `make.rs` script:

```bash
cargo install --path .
cargo wop make.rs precommit
```

Run `cargo wop make.rs help` to a see a list of available commands.

# Related projects

- [cargo-script][cargo-script] and forks of it [cargo-scripter][cargo-scripter],
  [cargo-eval][cargo-eval]
- [cargo-play][cargo-play]

[cargo-script]: https://github.com/DanielKeep/cargo-script
[cargo-eval]: https://github.com/reitermarkus/cargo-eval
[cargo-play]: https://crates.io/crates/cargo-play
[cargo-scripter]: https://crates.io/crates/cargo-scripter