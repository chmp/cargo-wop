//! Helper script to orchestrate build tasks
//!
//! Note this script requires cargo-wop to run. First install the cargo wop
//! package itself. Then run this script as `cargo wop make.rs`
//!
//! ```cargo
//! [dependencies]
//! anyhow = "1.0"
//! clap = "3.0.0-beta.2"
//! ```
use std::{ffi::OsStr, fmt::Debug, fs, path::Path, process::Command};

use anyhow::{ensure, Result};
use clap::Clap;

fn main() -> Result<()> {
    let task = Task::parse();

    match task {
        Task::Precommit => {
            run(&["rustfmt", "make.rs"])?;
            run(&["cargo", "fmt"])?;
            run(&["cargo", "wop", "build", "cargo-wop.rs"])?;
            // use cargo-wop to execute cargo-wop to test / write the manifest
            //
            // NOTE: use build instead of install, since install does not work
            // on windows as we cannot overwrite the cargo-wop executable as it
            // is running
            run(&["./cargo-wop", "wop", "test", "cargo-wop.rs"])?;
            run(&["./cargo-wop", "wop", "write-manifest", "cargo-wop.rs"])?;
            run(&["cargo", "test"])?;
            run(&["cargo", "build"])?;
        }
        Task::Clean => {
            run(&["cargo", "clean"])?;
            delete_file("./cargo-wop")?;
            delete_file("./cargo-wop.exe")?;
            delete_file("./cargo_wop.pdb")?;
        }
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

#[derive(Clap, Debug)]
enum Task {
    /// Run common tasks before committing.
    Precommit,
    /// Clean the project directory, removing any build files.
    Clean,
}
