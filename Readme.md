# `cargo-wop` - cargo without project

**WARNING:** this package is experimental at the moment.

The ambition is to allow running all cargo commands against a rust source with
additional dependency annotations. 

This project is heavily inspired by [cargo-script][cargo-script],
[cargo-eval][cargo-eval]. In contrast to these projects, `cargo-wop` aims to be
as close as possible to cargo and support all sensible arguments. 

Usage: 

```bash
# these two are the same
cargo wop my-script.rs
cargo wop run my-script.rs

# build  artifacts declared by the script and copy them into the working directory
cargo wop build my-script.rs

# Run the tests
cargo wop test my-script.rs
```
## How arguments are interpreted

At the moment the following cargo commands are supported: `bench`, `build`,
`check`, `clean`, `locate-project`, `metadata`, `pkgid`, `run`, `tree`, `test`,
`verify-project`. For most commands `cargo-wop` rewrites the command-line as
follows:

```bash
# Original command-line
cargo wop [cargo-command] [script] [args...]

# Rewritten commandline
cargo [cargo-command] --manifest-path [generated_manifest] [args...]
```

Some commands use additional rules:

- `run`: all arguments are passed per default to the script, not to cargo. To
  pass arguments to `cargo` place them before a `--`. For example: `cargo wop
  run my-script.rs --debug -- ...`
- `build`: build is executed twice. Once to build the package and a second time
  to determine the generated build artifacts and copy them into the local
  folder

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

# How-To's
## Building static C libraries

To build a static C library, simply specify a `[lib]` target with the correct
flags set:

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

# Related projects

- [cargo-script][cargo-script] and forks of it [cargo-scripter][cargo-scripter],
  [cargo-eval][cargo-eval]
- [cargo-play][cargo-play]

[cargo-script]: https://github.com/DanielKeep/cargo-script
[cargo-eval]: https://github.com/reitermarkus/cargo-eval
[cargo-play]: https://crates.io/crates/cargo-play
[cargo-scripter]: https://crates.io/crates/cargo-scripter