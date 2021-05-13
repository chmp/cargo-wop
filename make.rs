//! Helper script to orchestrate build tasks
//!
//! Note this script requires cargo-wop to run. First install the cargo wop
//! package itself. Then run this script as `cargo wop make.rs`
//!
//! ```cargo
//! [dependencies]
//! anyhow = "1.0"
//! ```
use std::{ffi::OsStr, fmt::Debug, fs, path::Path, process::Command};

use anyhow::{ensure, Result};

fn main() -> Result<()> {
    let task = single(std::env::args().skip(1));

    match task.as_str() {
        "precommit" => {
            run(&["rustfmt", "make.rs"])?;
            run(&["cargo", "fmt"])?;
            run(&["cargo", "wop", "build", "cargo-wop.rs"])?;
            // use cargo-wop to execute cargo-wop to test / write the manifest
            //
            // NOTE: use build instead of install, since install does not work
            // on windows as we cannot overwrite the cargo-wop executable while
            // it is running
            let cargo_wop_exe = if std::env::consts::FAMILY == "windows" {
                "./cargo-wop.exe"
            } else {
                "./cargo-wop"
            };

            run(&[cargo_wop_exe, "wop", "test", "cargo-wop.rs"])?;
            run(&[cargo_wop_exe, "wop", "write-manifest", "cargo-wop.rs"])?;
            run(&["cargo", "test"])?;
            run(&["cargo", "build"])?;
        }
        "clean" => {
            run(&["cargo", "clean"])?;
            delete_file("./cargo-wop")?;
            delete_file("./cargo-wop.exe")?;
            delete_file("./cargo_wop.pdb")?;
        }
        _ => panic!("Unknown task {}", task),
    }
    Ok(())
}

fn run<S: AsRef<OsStr> + Debug>(args: &[S]) -> Result<()> {
    ensure!(!args.is_empty(), "Cannot run empty args");
    println!(":: {:?}", args);
    let status = Command::new(args[0].as_ref()).args(&args[1..]).status()?;

    ensure!(status.success(), "Command {:?} failed", args);
    Ok(())
}

fn delete_file<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn single<I: Iterator>(mut it: I) -> I::Item {
    let res = it.next().expect("Need at least one item");
    if it.next().is_some() {
        panic!("Trailing arguments");
    }
    res
}
