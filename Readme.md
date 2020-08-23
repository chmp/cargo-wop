# cargo-wop - cargo without project

The ambition is to allow running all cargo commands against a rust source with
additional dependency annotations. 

This project is heavily inspired by [cargo-script][cargo-script] and
[cargo-eval][cargo-eval].

Usage: 

```bash
# these two are the same
cargo wop myscript.rs
cargo wop run myscript.rs

# build  artifacts declared by the script and copy them into the working directory
cargo wop build myscript.rs
```

[cargo-script]: https://github.com/DanielKeep/cargo-script
[cargo-eval]: https://github.com/reitermarkus/cargo-eval
