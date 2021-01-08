# `cargo-wop` - cargo without project

**WARNING:** this package is experimental at the moment.

Rust source files as self-contained projects. `cargo-wop` allows to `cargo` work
with rust source file as if thy were full projects.   This project is heavily
inspired by [cargo-script][cargo-script], [cargo-eval][cargo-eval]. In contrast
to these projects, `cargo-wop` is designed to be as close as possible to cargo
and support all sensible arguments.

Run a file as a script:

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
rewrites the command-line as follows:

```bash
# Original command-line
cargo wop [cargo-command] [script] [args...]

# Rewritten command line
cargo [cargo-command] --manifest-path [generated_manifest] [args...]
```

Some commands use additional rules:

- `run`: all arguments are passed per default to the script, not to cargo. To
  pass arguments to `cargo` place them before a `--`. For example: `cargo wop
  run my-script.rs --debug -- ...`
- `build`: is executed twice. Once to build the package and a second time to
  determine the generated build artifacts and copy them into the local folder
- `build` and `run` default to release builds. To disable this behavior, use the
  `build-debug` and `run-debug` commands.
- `install`: no manifest path is added, but the `--path` argument to the
  manifest directory

Custom commands:

- `exec` execute the command after the source inside the manifest directory
- `manifest`: print out the generated manifest

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