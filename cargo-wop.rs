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
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{anyhow, bail, ensure, Result};
use serde_json::{self, Value as JsonValue};
use sha1::Sha1;
use toml::{self, Value};

fn main() -> Result<()> {
    std::process::exit(main_impl()?);
}

fn main_impl() -> Result<i32> {
    let args = parse_args(std::env::args_os().skip(1))?;
    let res = execute_args(args, std::env::current_dir()?)?;
    Ok(res)
}

/// Parse the command line arguments
///
fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Args> {
    let args = args.collect::<Vec<_>>();
    ensure!(
        args.len() >= 2,
        "Need at least two  arguments: <wop [source-file]> or <wop [command] [source-file]>"
    );
    ensure!(args[0] == "wop", "First argument must be wop");

    let (command, target, rest_args) = if has_extension(args[1].as_os_str()) {
        (String::from("run"), PathBuf::from(&args[1]), &args[2..])
    } else {
        ensure!(
            args.len() >= 3,
            "Need at least three arguments: <wop [command] [source-file]>"
        );
        (
            to_utf8_string(&args[1])?,
            PathBuf::from(&args[2]),
            &args[3..],
        )
    };

    let result = match command.as_str() {
        "manifest" => {
            ensure!(
                rest_args.is_empty(),
                "The manifest command does not take additional arguments"
            );
            Args::Manifest(target)
        }
        "exec" => {
            ensure!(!rest_args.is_empty(), "Need at least an argument");
            let exec = Exec {
                target: target,
                command: rest_args[0].clone(),
                args: rest_args[1..].to_vec(),
            };
            Args::Exec(exec)
        }
        _ if is_cargo_command(&command) => CargoCall::new(command, target)
            .with_args(rest_args)
            .normalize()?
            .into_args(),
        _ => bail!("Unknown command: {}", command),
    };
    Ok(result)
}

fn to_utf8_string(s: &OsStr) -> Result<String> {
    let result = s
        .to_str()
        .ok_or_else(|| anyhow!("Could not get utf8 rep"))?
        .to_owned();
    Ok(result)
}

fn has_extension(s: &OsStr) -> bool {
    AsRef::<Path>::as_ref(s).extension().is_some()
}

fn is_cargo_command(command: &str) -> bool {
    match command {
        "bench" | "build" | "build-debug" | "check" | "clean" | "clippy" | "install"
        | "locate-project" | "metadata" | "pkgid" | "run" | "run-debug" | "tree" | "test"
        | "verify-project" => true,
        _ => false,
    }
}

fn execute_args(args: Args, refpath: impl AsRef<Path>) -> Result<i32> {
    let refpath = refpath.as_ref();

    match args {
        Args::GenericCargoCall(call) => {
            let project_info = prepare_manifest_dir(&call.target, refpath)?;
            let exit_code = call.execute(&project_info)?;
            Ok(exit_code)
        }
        Args::BuildCargoCall(call) => {
            let project_info = prepare_manifest_dir(&call.target, refpath)?;
            let result = call.execute(&project_info)?;
            ensure!(
                result == 0,
                "Error during build. Cannot copy build artifacts"
            );
            let artifacts = collect_build_artifacts(&call, &project_info)?;
            copy_build_artifacts(artifacts, std::env::current_dir()?)?;
            Ok(0)
        }
        Args::InstallCargoCall(call) => {
            let project_info = prepare_manifest_dir(&call.target, refpath)?;
            let mut command = Command::new("cargo");
            command
                .arg(call.command.as_str())
                .arg("--path")
                .arg(&project_info.manifest_dir)
                .args(call.args.iter());

            let exit_code = command.status()?.code().unwrap_or_default();
            Ok(exit_code)
        }
        Args::Manifest(target) => {
            // TODO: remove the duplication of file + parse + normalize?
            let file = File::open(target.as_path())?;
            let manifest = parse_manifest(file)?;
            let manifest = normalize_manifest(manifest, target.as_path(), refpath)?;
            print!("{}", toml::to_string(&manifest)?);
            Ok(0)
        }
        Args::Exec(exec) => {
            let project_info = prepare_manifest_dir(exec.target.as_path(), refpath)?;
            let mut command = Command::new(&exec.command);
            command
                .args(exec.args.iter().cloned())
                .current_dir(&project_info.manifest_dir.canonicalize()?);

            let exit_code = command.status()?.code().unwrap_or_default();
            Ok(exit_code)
        }
    }
}

/// Prepare the cargo project directory
///
/// This commands writes the manifest and copies the source file. After this
/// step, cargo calls can be made against this directory.
///
fn prepare_manifest_dir(target: impl AsRef<Path>, refpath: impl AsRef<Path>) -> Result<ProjectInfo> {
    let target = refpath.as_ref().join(target);
    let manifest_dir = find_project_dir(target.as_path())?;
    let manifest_path = manifest_dir.join("Cargo.toml");

    let source_path = target
        .file_name()
        .ok_or_else(|| anyhow!("Cannot handle directory source"))?;
    let source_path = manifest_dir.join(source_path);

    // TODO: get the name from the normalized manifest in case the user has overwritten it
    let name = to_utf8_string(
        target
            .file_stem()
            .ok_or_else(|| anyhow!("Could not get name"))?,
    )?;

    let manifest = parse_manifest_file(target.as_path())?;
    let manifest = normalize_manifest(manifest, target.as_path(), refpath)?;

    // perform any faillible operations
    fs::create_dir_all(&manifest_dir)?;
    fs::write(&manifest_path, toml::to_string(&manifest)?)?;
    fs::copy(target.as_path(), source_path.as_path())?;

    Ok(ProjectInfo {
        manifest_path,
        manifest_dir,
        name,
    })
}

/// Execute a cargo call and collect any generated artifacts
///
fn collect_build_artifacts(call: &CargoCall, project_info: &ProjectInfo) -> Result<Vec<String>> {
    let output = call
        .build_with_args(&project_info, &["--message-format", "json"])
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

fn copy_build_artifacts<I, P, T>(from: I, to: T) -> Result<()>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
    T: AsRef<Path>,
{
    for src in from {
        let src = src.as_ref();
        let dst = to.as_ref().join(
            src.file_name()
                .ok_or_else(|| anyhow!("Invalid source filename"))?,
        );

        fs::copy(src, dst)?;
    }
    Ok(())
}

struct ProjectInfo {
    name: String,
    manifest_path: PathBuf,
    manifest_dir: PathBuf,
}

#[derive(Debug, PartialEq)]
enum Args {
    /// Execute a direct cargo command
    GenericCargoCall(CargoCall),
    /// A build step
    ///
    /// It can safely be executed a second time with `--message-format json` to
    /// get the build artifacts
    BuildCargoCall(CargoCall),
    /// A install step that gets passed the manifest dir not the file
    InstallCargoCall(CargoCall),
    /// Print out the manifest
    Manifest(PathBuf),
    /// Execute a command inside the manifest dir
    Exec(Exec),
}

#[derive(Debug, PartialEq)]
struct CargoCall {
    command: String,
    target: PathBuf,
    args: Vec<OsString>,
}

impl CargoCall {
    /// Execute a cargo call
    ///
    fn execute(&self, project_info: &ProjectInfo) -> Result<i32> {
        let exit_code = self
            .build_with_args::<&str>(&project_info, &[])
            .status()?
            .code()
            .unwrap_or_default();
        Ok(exit_code)
    }

    fn build_with_args<S: AsRef<OsStr>>(
        &self,
        project_info: &ProjectInfo,
        extra_args: &[S],
    ) -> Command {
        let mut result = Command::new("cargo");
        result
            .arg(self.command.as_str())
            .arg("--manifest-path")
            .arg(project_info.manifest_path.as_os_str())
            .args(extra_args)
            .args(self.args.iter());

        result
    }
}

#[derive(Debug, PartialEq)]
struct Exec {
    command: OsString,
    target: PathBuf,
    args: Vec<OsString>,
}

impl CargoCall {
    fn new<Command, Target>(command: Command, target: Target) -> Self
    where
        Command: Into<String>,
        Target: Into<PathBuf>,
    {
        Self {
            command: command.into(),
            target: target.into(),
            args: Vec::new(),
        }
    }

    fn with_args<Args, Arg>(mut self, args: Args) -> Self
    where
        Args: IntoIterator<Item = Arg>,
        Arg: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(|arg| arg.into()));
        self
    }

    /// Normalize the arguments
    fn normalize(mut self) -> Result<Self> {
        let (cargo_args, commands_args) = self.split_args();

        let mut cargo_args = cargo_args.to_owned();
        if let "build" | "run" = self.command.as_str() {
            cargo_args.push(OsString::from("--release"));
        }

        self.args = if commands_args.is_empty() {
            cargo_args
        } else {
            let mut new_args = cargo_args;
            new_args.push(OsString::from("--"));
            new_args.extend(commands_args.iter().cloned());
            new_args
        };

        self.command = match self.command.as_str() {
            "build-debug" => String::from("build"),
            "run-debug" => String::from("run"),
            _ => self.command,
        };

        Ok(self)
    }

    /// split the arguments into (cargo, command args)
    fn split_args(&self) -> (&[OsString], &[OsString]) {
        if self.command != "run" {
            (self.args.as_slice(), &[])
        } else {
            let splitter = self.args.iter().position(|s| s == "--");
            if let Some(splitter) = splitter {
                (&self.args[..splitter], &self.args[(splitter + 1)..])
            } else {
                (&[], self.args.as_slice())
            }
        }
    }

    fn into_args(self) -> Args {
        match self.command.as_str() {
            "build" => Args::BuildCargoCall(self),
            "install" => Args::InstallCargoCall(self),
            _ => Args::GenericCargoCall(self),
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

    let env_var = if std::env::consts::OS == "windows" {
        "HOME"
    } else {
        "USERPROFILE"
    };

    // TODO: handle windows
    if let Some(user_home) = std::env::var_os(env_var) {
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
fn normalize_manifest(mut manifest: Value, target_path: impl AsRef<Path>, refpath: impl AsRef<Path>) -> Result<Value> {
    let target_path = target_path.as_ref();
    let project_dir = refpath.as_ref().join(target_path)
        .parent()
        .ok_or_else(|| anyhow!("Invalid file path"))?
        .to_owned();
    let (target_path, target_name) = get_path_info(target_path)?;

    let root = manifest
        .as_table_mut()
        .ok_or_else(|| anyhow!("Can only handle manifests that are tables"))?;

    ensure_valid_package(root, &target_name)?;
    ensure_at_least_a_single_target(root)?;

    patch_all_targets(root, &target_path, &target_name)?;
    patch_all_dependencies(root, &project_dir, refpath)?;

    Ok(manifest)
}

/// Helper for normalize_manifest: Get the file name and stem from a path
///
fn get_path_info(path: impl AsRef<Path>) -> Result<(String, String)> {
    let path = path.as_ref();

    let target_name = to_utf8_string(
        path.file_stem()
            .ok_or_else(|| anyhow!("Cannot build manifest for non file target"))?,
    )?;
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

/// Patch all available target definition
fn patch_all_targets(
    root: &mut toml::map::Map<String, Value>,
    path: &str,
    name: &str,
) -> Result<()> {
    if let Some(lib) = root.get_mut("lib") {
        patch_target(lib, &path, &name)?;
    }

    if let Some(bins) = root.get_mut("bin") {
        let bins = bins
            .as_array_mut()
            .ok_or_else(|| anyhow!("Invalid manifest: bin not an array"))?;

        for bin in bins {
            patch_target(bin, &path, &name)?;
        }
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

/// Replace all path dependencies with absolute paths
///
fn patch_all_dependencies(
    root: &mut toml::map::Map<String, Value>,
    project_dir: &Path,
    refpath: impl AsRef<Path>
) -> Result<()> {
    let refpath = refpath.as_ref();
    for dep_root_key in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(dep_root) = root.get_mut(*dep_root_key) {
            let dep_root = dep_root
                .as_table_mut()
                .ok_or_else(|| anyhow!("Invalid manifest {} not an array", *dep_root_key))?;

            for dep in dep_root {
                let dep = dep.1;
                let dep = match dep.as_table_mut() {
                    Some(dep) => dep,
                    None => continue,
                };

                if !dep.contains_key("path") {
                    continue;
                }

                let path = dep
                    .get("path")
                    .unwrap()
                    .as_str()
                    .ok_or_else(|| anyhow!("Invalid manifest: non string path"))?;
                let path = refpath.join(project_dir).join(path);
                let path = path.canonicalize()?;
                let path = path
                    .to_str()
                    .ok_or_else(|| anyhow!("Cannot interpret dependency path a string"))?;
                dep.insert(String::from("path"), path.into());
            }
        }
    }

    Ok(())
}

fn parse_manifest_file(path: impl AsRef<Path>) -> Result<Value> {
    let file = File::open(path)?;
    parse_manifest(file)
}

/// Parse the manifest from the initial doc comment
///
fn parse_manifest(reader: impl Read) -> Result<Value> {
    let reader = BufReader::new(reader);

    let mut state = ParseState::Start;
    let mut result = String::new();

    for line in reader.lines() {
        let line = line?;
        let line_start = LineStart::from(line.as_str());

        state = match (state, line_start) {
            (ParseState::Start, LineStart::Other) => return Ok(Value::Table(Default::default())),
            (ParseState::DocComment, LineStart::Other) => {
                return Ok(Value::Table(Default::default()))
            }
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
            (ParseState::Manifest, LineStart::ManifestEnd) => return Ok(toml::from_str(&result)?),
            (ParseState::Manifest, LineStart::ManifestStart) => bail!("Invalid manifest"),
            (ParseState::Manifest, LineStart::Other) => bail!("Invalid manifest"),
        };
    }

    if state == ParseState::Manifest {
        bail!("Incomplete manifest");
    }

    return Ok(Value::Table(Default::default()));

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
    use std::ffi::OsString;

    fn parse_args(args: &[&str]) -> Result<Args> {
        let mut os_args = Vec::<OsString>::new();
        for arg in args {
            os_args.push(OsString::from(*arg));
        }

        super::parse_args(os_args.into_iter())
    }

    #[test]
    fn example_implicit_run() {
        assert_eq!(
            parse_args(&["wop", "example.rs"]).unwrap(),
            CargoCall::new("run", "example.rs")
                .with_args(&["--release"])
                .into_args()
        );
    }

    #[test]
    fn example_implicit_run_debug() {
        assert_eq!(
            parse_args(&["wop", "example.rs"]).unwrap(),
            CargoCall::new("run", "example.rs",)
                .with_args(&["--release"])
                .into_args()
        );
    }

    #[test]
    fn example_run_debug() {
        assert_eq!(
            parse_args(&["wop", "run-debug", "example.rs"]).unwrap(),
            CargoCall::new("run", "example.rs").into_args()
        );
    }

    #[test]
    fn example2() {
        let actual = parse_args(&["wop", "build", "example.rs"]).unwrap();
        let expected = CargoCall::new("build", "example.rs")
            .with_args(&["--release"])
            .into_args();

        assert_eq!(actual, expected);
    }
}

#[cfg(test)]
mod test_parse_manifest {
    use super::parse_manifest;
    use anyhow::Result;

    const EXAMPLE: &str = r#"//! cargo-wop
//!
//! ```cargo
//! [dependencies]
//! anyhow = "1.0"
//! sha1 = "0.6.0"
//! ```
//!

use std::fs;"
"#;

    const EXAMPLE_MANIFEST: &str = r#"
        [dependencies]
        anyhow = "1.0"
        sha1 = "0.6.0"
    "#;

    #[test]
    fn example() -> Result<()> {
        let actual = parse_manifest(EXAMPLE.as_bytes())?;
        let expected = toml::from_str(EXAMPLE_MANIFEST)?;

        assert_eq!(actual, expected);
        Ok(())
    }
}
