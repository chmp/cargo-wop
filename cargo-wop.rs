//! cargo-wop
//!
//! ```cargo
//! [dependencies]
//! anyhow = "1.0"
//! serde_json = "1.0"
//! sha1 = "0.6.0"
//! toml = "0.5"
//! ```
//!
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{anyhow, bail, ensure, Result};
use serde_json::{self, Value as JsonValue};
use sha1::Sha1;
use toml::{self, Value};

fn main() -> Result<()> {
    let args = parse_args(std::env::args_os().skip(1).collect())?;

    match args {
        Args::GenericCargoCall(call) => {
            let project_info = prepare_cargo_call(&call)?;
            execute_cargo_call(&call, &project_info)?;
        }
        Args::BuildCargoCall(call) => {
            let project_info = prepare_cargo_call(&call)?;
            execute_cargo_call(&call, &project_info)?;
            let artifacts = collect_build_artifacts(&call, &project_info)?;

            // TODO: copy the build artifacts into the current directory

            println!("Generated artifacts: {:?}", artifacts);
        }
    }

    Ok(())
}

/// Parse the command line arguments
///
fn parse_args(args: Vec<OsString>) -> Result<Args> {
    ensure!(args.len() >= 1, "Need at least a single argument");

    let first_path = AsRef::<Path>::as_ref(&args[0]);
    if first_path.extension().is_some() {
        let first_path = first_path.to_owned();
        let result = CargoCall::new(
            String::from("run"),
            first_path,
            args.into_iter().skip(1).collect(),
        );
        return Ok(result.to_args());
    }

    let command = args[0]
        .to_str()
        .ok_or_else(|| anyhow!("Cannot interpret first argument as string"))?;

    if is_cargo_command(command) {
        ensure!(
            args.len() >= 2,
            "Need at least two arguments for a cargo command"
        );

        // TODO: modify args to parse debug flag, insert release otherwise
        // TODO: modify args to insert cargo / command separators (by using --)

        let result = CargoCall::new(
            command.to_owned(),
            PathBuf::from(args[1].to_owned()),
            args.iter().skip(2).cloned().collect(),
        );
        return Ok(result.to_args());
    }

    bail!("Unknown command: {}", command);
}

fn is_cargo_command(command: &str) -> bool {
    match command {
        "run" | "build" | "test" => true,
        _ => false,
    }
}

/// Prepare the cargo project directory
///
/// This commands writes the manifest and copies the source file. After this
/// step, cargo calls can be made against this directory.
///
fn prepare_cargo_call(call: &CargoCall) -> Result<ProjectInfo> {
    let target = call.target.canonicalize()?;
    let file = File::open(target.as_path())?;

    let project_dir = find_project_dir(target.as_path())?;

    let manifest = parse_manifest(file)?;

    let manifest: Value = toml::from_str(manifest.as_str())?;
    let manifest = normalize_manifest(manifest, target.as_path())?;
    let manifest = toml::to_string(&manifest)?;

    fs::create_dir_all(&project_dir)?;

    let manifest_path = project_dir.join("Cargo.toml");
    let mut file = File::create(manifest_path.clone())?;
    file.write_all(manifest.as_bytes())?;

    let file = project_dir.join(target.file_name().unwrap());
    fs::copy(target.as_path(), file)?;

    // TODO: get the name from the normalized manifest in case the user has overwritten it
    let name = target
        .file_stem()
        .ok_or_else(|| anyhow!("Could not get name"))?
        .to_str()
        .ok_or_else(|| anyhow!("Could not get utf8 rep"))?
        .to_owned();

    Ok(ProjectInfo {
        manifest_path,
        name,
    })
}

/// Execute a cargo call
///
fn execute_cargo_call(call: &CargoCall, project_info: &ProjectInfo) -> Result<()> {
    let exit_code = build_cargo_command(&call, &project_info, &[])
        .status()?
        .code()
        .unwrap_or_default();

    ensure!(
        exit_code == 0,
        "Error during running cargo. Exit code {}",
        exit_code
    );

    Ok(())
}

/// Execute a cargo call and collect any generated artifacts
///
fn collect_build_artifacts(call: &CargoCall, project_info: &ProjectInfo) -> Result<Vec<String>> {
    let output = build_cargo_command(&call, &project_info, &["--message-format", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    let exit_code = output.status.code().unwrap_or_default();
    ensure!(
        exit_code == 0,
        "Error during running cargo. Exit code {}",
        exit_code
    );

    let artifacts = parse_build_output(output.stdout.as_slice(), &project_info)?;
    Ok(artifacts)
}

/// Parse the output of a cargo build step
///
fn parse_build_output(output: &[u8], project_info: &ProjectInfo) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let reader = BufReader::new(output);
    for line in reader.lines() {
        let line = line?;
        let value: JsonValue = serde_json::from_str(&line)?;

        let reason = value
            .get("reason")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| anyhow!("Invalid cargo output reason not a string"))?;

        if reason != "compiler-artifact" {
            continue;
        }

        let package_id = value
            .get("package_id")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| anyhow!("Invalid compiler-artifact: package_id not a string"))?;

        let needle = format!("{} ", project_info.name);
        if !package_id.starts_with(&needle) {
            continue;
        }

        let filenames = value
            .get("filenames")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| anyhow!("Invalid compiler-artifact: filenames not an array"))?;

        for filename in filenames {
            let filename = filename
                .as_str()
                .ok_or_else(|| anyhow!("Invalid file name not a string"))?;
            result.push(filename.to_owned());
        }
    }
    Ok(result)
}

/// A helper to build a cargo call command
///
fn build_cargo_command(
    call: &CargoCall,
    project_info: &ProjectInfo,
    extra_args: &[&str],
) -> Command {
    let mut result = Command::new("cargo");

    result
        .arg(call.command.as_str())
        .arg("--manifest-path")
        .arg(project_info.manifest_path.as_os_str())
        .args(extra_args)
        .args(call.args.iter());

    result
}

struct ProjectInfo {
    name: String,
    manifest_path: PathBuf,
}

#[derive(Debug, PartialEq)]
enum Args {
    /// Execute a direct cargo command
    GenericCargoCall(CargoCall),
    BuildCargoCall(CargoCall),
}

#[derive(Debug, PartialEq)]
struct CargoCall {
    command: String,
    target: PathBuf,
    args: Vec<OsString>,
}

impl CargoCall {
    fn new(command: String, target: PathBuf, args: Vec<OsString>) -> Self {
        Self {
            command,
            target,
            args,
        }
    }

    fn to_args(self) -> Args {
        if self.command != "build" {
            Args::GenericCargoCall(self)
        } else {
            Args::BuildCargoCall(self)
        }
    }
}

/// Find the project directory from the supplied file
///
fn find_project_dir(source: impl AsRef<Path>) -> Result<PathBuf> {
    let source = source.as_ref();

    let target_name = source
        .file_stem()
        .ok_or_else(|| anyhow!("Could not get path stem"))?;
    let mut target_name = target_name.to_owned();
    target_name.push("-");
    target_name.push(&hash_path(source));

    let mut result = find_cache_dir()?;
    result.push(target_name);

    Ok(result)
}

/// Find the internal cache dir for cargo-wop
///
fn find_cache_dir() -> Result<PathBuf> {
    let mut result = find_cargo_home_dir()?;
    result.push("wop-cache");
    Ok(result)
}

/// Find the cargo home
///
/// Follow the documentation found
/// [here](https://doc.rust-lang.org/cargo/reference/environment-variables.html).
///
fn find_cargo_home_dir() -> Result<PathBuf> {
    if let Some(cargo_home) = std::env::var_os("CARGO_HOME") {
        let cargo_home = PathBuf::from(cargo_home);
        return Ok(cargo_home);
    }

    // TODO: handle windows
    if let Some(user_home) = std::env::var_os("HOME") {
        let mut user_home = PathBuf::from(user_home);
        user_home.push(".cargo");
        return Ok(user_home);
    }

    bail!("Could not determine cargo home directory");
}

fn hash_path(path: impl AsRef<Path>) -> String {
    let mut hash = Sha1::new();
    hash.update(path.as_ref().to_string_lossy().as_bytes());
    let digest = hash.digest();
    let res = digest.to_string();
    res[..8].to_string()
}

/// Normalize the embedded manifest that it can be used to build the target
///
/// This function inserts a package section with name, version, and edition. It
/// makes sure at least a single target exists (a binary as a default). For each
/// target it sets the correct path to the source file.
///
fn normalize_manifest(mut manifest: Value, target_path: impl AsRef<Path>) -> Result<Value> {
    let (target_path, target_name) = get_path_info(target_path)?;

    let root = manifest
        .as_table_mut()
        .ok_or_else(|| anyhow!("Can only handle manifests that are tables"))?;

    ensure_valid_package(root, &target_name)?;
    ensure_at_least_a_single_target(root)?;

    // TODO: rewrite depdendencies by replacing relative paths with absolute ones

    // TODO: refactor this into its own function
    if let Some(lib) = root.get_mut("lib") {
        patch_target(lib, &target_path, &target_name)?;
    }

    if let Some(bins) = root.get_mut("bin") {
        let bins = bins
            .as_array_mut()
            .ok_or_else(|| anyhow!("Invalid manifest: bin not an array"))?;

        for bin in bins {
            patch_target(bin, &target_path, &target_name)?;
        }
    }

    Ok(manifest)
}

/// Helper for normalize_manifest: Get the file name and stem from a path
///
fn get_path_info(path: impl AsRef<Path>) -> Result<(String, String)> {
    let path = path.as_ref();

    let target_name = path
        .file_stem()
        .ok_or_else(|| anyhow!("Cannot build manifest for non file target"))?
        .to_str()
        .ok_or_else(|| anyhow!("Cannot build manifest for paths not-expressible in utf-8"))?
        .to_owned();
    let target_path = path
        .file_name()
        .ok_or_else(|| anyhow!("Cannot build manifest for non file target"))?
        .to_str()
        .ok_or_else(|| anyhow!("Cannot build manifest for paths not-expressible in utf-8"))?
        .to_owned();

    Ok((target_path, target_name))
}

/// Helper for normalize_manifest: Ensure the package table is correctly filled
///
fn ensure_valid_package(root: &mut toml::map::Map<String, Value>, name: &str) -> Result<()> {
    if !root.contains_key("package") {
        root.insert(String::from("package"), Value::Table(Default::default()));
    }
    let package = root
        .get_mut("package")
        .unwrap()
        .as_table_mut()
        .ok_or_else(|| anyhow!("Invalid manifest: package is not a table"))?;

    if !package.contains_key("name") {
        package.insert(String::from("name"), Value::from(name));
    }
    if !package.contains_key("version") {
        package.insert(String::from("version"), Value::from("0.1.0"));
    }

    if !package.contains_key("edition") {
        package.insert(String::from("edition"), Value::from("2018"));
    }

    Ok(())
}

/// Helper for normalize manifest: Ensure at least a single target is available
///
fn ensure_at_least_a_single_target(root: &mut toml::map::Map<String, Value>) -> Result<()> {
    let has_single_bin = root.contains_key("bin")
        && root
            .get("bin")
            .and_then(|b| b.as_array())
            .map(|b| !b.is_empty())
            .ok_or_else(|| anyhow!("Invalid manifest"))?;
    let has_definition = root.contains_key("lib") || has_single_bin;

    if has_definition {
        return Ok(());
    }

    root.insert(String::from("bin"), Value::Array(Default::default()));

    let bins = root
        .get_mut("bin")
        .unwrap()
        .as_array_mut()
        .ok_or_else(|| anyhow!("Invalid manifest: bin is not an array"))?;
    if bins.is_empty() {
        bins.push(Value::Table(Default::default()));
    }

    Ok(())
}

/// Helper for normalize manifest: patch the target definition to use the correct file path
fn patch_target(target: &mut Value, path: &str, name: &str) -> Result<()> {
    let bin = target
        .as_table_mut()
        .ok_or_else(|| anyhow!("Cannot patch non table target"))?;
    bin.insert(String::from("path"), Value::String(path.to_owned()));

    if !bin.contains_key("name") {
        bin.insert(String::from("name"), Value::String(name.to_owned()));
    }

    Ok(())
}

/// Parse the manifest from the initial doc comment
///
fn parse_manifest(reader: impl Read) -> Result<String> {
    let reader = BufReader::new(reader);

    let mut state = ParseState::Start;
    let mut result = String::new();

    for line in reader.lines() {
        let line = line?;
        let line_start = LineStart::from(line.as_str());

        state = match (state, line_start) {
            (ParseState::Start, LineStart::Other) => return Ok(String::new()),
            (ParseState::DocComment, LineStart::Other) => return Ok(String::new()),
            (ParseState::Start, LineStart::DocComment) => ParseState::DocComment,
            (ParseState::Start, LineStart::ManifestEnd) => state,
            (ParseState::DocComment, LineStart::DocComment) => state,
            (ParseState::DocComment, LineStart::ManifestEnd) => state,
            (ParseState::Start, LineStart::ManifestStart) => ParseState::Manifest,
            (ParseState::DocComment, LineStart::ManifestStart) => ParseState::Manifest,

            (ParseState::Manifest, LineStart::DocComment) => {
                let line = line.strip_prefix("//!").unwrap().trim_start();
                result.push_str(line);
                result.push('\n');
                state
            }
            (ParseState::Manifest, LineStart::ManifestEnd) => return Ok(result),
            (ParseState::Manifest, LineStart::ManifestStart) => bail!("Invalid manifest"),
            (ParseState::Manifest, LineStart::Other) => bail!("Invalid manifest"),
        };
    }

    if state == ParseState::Manifest {
        bail!("Incomplete manifest");
    }

    return Ok(String::new());

    #[derive(Debug, PartialEq, Clone, Copy)]
    enum ParseState {
        Start,
        DocComment,
        Manifest,
    }

    #[derive(Debug, PartialEq, Clone, Copy)]
    enum LineStart {
        DocComment,
        ManifestStart,
        ManifestEnd,
        Other,
    }

    impl From<&str> for LineStart {
        fn from(line: &str) -> Self {
            if line.starts_with("//! ```cargo") {
                Self::ManifestStart
            } else if line.starts_with("//! ```") {
                Self::ManifestEnd
            } else if line.starts_with("//!") {
                Self::DocComment
            } else {
                Self::Other
            }
        }
    }
}

#[cfg(test)]
mod test_parse_args {
    use super::{Args, CargoCall};
    use anyhow::Result;
    use std::{ffi::OsString, path::PathBuf};

    fn parse_args(args: &[&str]) -> Result<Args> {
        let mut os_args = Vec::<OsString>::new();
        for arg in args {
            os_args.push(OsString::from(*arg));
        }

        super::parse_args(os_args)
    }

    #[test]
    fn example() -> Result<()> {
        let actual = parse_args(&["example.rs"])?;
        let expected =
            CargoCall::new(String::from("run"), PathBuf::from("example.rs"), Vec::new()).to_args();

        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn example2() -> Result<()> {
        let actual = parse_args(&["build", "example.rs"])?;
        let expected = CargoCall::new(
            String::from("build"),
            PathBuf::from("example.rs"),
            Vec::new(),
        )
        .to_args();

        assert_eq!(actual, expected);
        Ok(())
    }
}

#[cfg(test)]
mod test_parse_manifest {
    use super::parse_manifest;
    use anyhow::Result;

    #[test]
    fn example() -> Result<()> {
        let source = concat!(
            r#"//! cargo-wop"#,
            "\n",
            r#"//!"#,
            "\n",
            r#"//! ```cargo"#,
            "\n",
            r#"//! [dependencies]"#,
            "\n",
            r#"//! anyhow = "1.0""#,
            "\n",
            r#"//! sha1 = "0.6.0""#,
            "\n",
            r#"//! ```"#,
            "\n",
            r#"//!"#,
            "\n",
            r#""#,
            "\n",
            r#"use std::fs;"#,
            "\n",
        );
        let actual = parse_manifest(source.as_bytes())?;

        let expected = concat!(
            r#"[dependencies]"#,
            "\n",
            r#"anyhow = "1.0""#,
            "\n",
            r#"sha1 = "0.6.0""#,
            "\n",
        );
        let expected = String::from(expected);

        assert_eq!(actual, expected);
        Ok(())
    }
}
