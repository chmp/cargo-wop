//! Helper script to orchestrate build tasks
//!
//! Note this script requires cargo-wop to run. First install the cargo wop
//! package itself. The run this script as `cargo wop make.rs`
//!
//! ```cargo
//! [dependencies]
//! anyhow = "1.0"
//! clap = "3.0.0-beta.2"
//! ```
use std::{ffi::OsStr, fmt::Debug, process::Command};

use anyhow::{ensure, Result};
use clap::Clap;

fn main() -> Result<()> {
    let task = Task::parse();

    match task {
        Task::Precommit => {
            run(&["cargo", "fmt"])?;
            run(&["cargo", "test"])?;
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

#[derive(Clap, Debug)]
enum Task {
    Precommit,
}
