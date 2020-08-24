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
cargo wop myscript.rs
cargo wop run myscript.rs

# build  artifacts declared by the script and copy them into the working directory
cargo wop build myscript.rs

# Run the tests
cargo wop test myscript.rs
```
## How arguments are interpreted

## Specifying dependencies

## Related projects

- [cargo-script][cargo-script] and forks of it [cargo-scripter][cargo-scripter],
  [cargo-eval][cargo-eval]
- [cargo-play][cargo-play]

[cargo-script]: https://github.com/DanielKeep/cargo-script
[cargo-eval]: https://github.com/reitermarkus/cargo-eval
[cargo-play]: https://crates.io/crates/cargo-play
[cargo-scripter]: https://crates.io/crates/cargo-scripter