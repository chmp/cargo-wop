//! cargo-wop
//!
//! ```cargo
//! [package]
//! name = "cargo-wop"
//! version = "0.1.6"
//! authors = ["Christopher Prohm"]
//! edition = "2018"
//!
//! repository = "https://github.com/chmp/cargo-wop"
//! description = "Cargo for single-file projects"
//! readme = "Readme.md"
//! license = "MIT"
//!
//! [[bin]]
//! name = "cargo-wop"
//! path = "cargo-wop.rs"
//!
//! [dependencies]
//! anyhow = "1.0"
//! serde_json = "1.0"
//! sha1 = "0.6.0"
//! toml = { version = "0.5", features = ["preserve_order"] }
//!
//! [cargo-wop]
//! filter = {  "cargo_wop.pdb" = "" }
//! ```
//!
use anyhow::Result;

use argparse::parse_args;
use execution::execute_args;
use execution_env::StdExecutionEnv;

fn main() -> Result<()> {
    std::process::exit(main_impl()?);
}

fn main_impl() -> Result<i32> {
    let env = StdExecutionEnv::new()?;
    let args = parse_args(std::env::args_os().skip(1))?;
    let res = execute_args(args, &env)?;
    Ok(res)
}

mod argparse {
    use anyhow::{anyhow, bail, ensure, Result};
    use std::{
        ffi::{OsStr, OsString},
        path::{Path, PathBuf},
    };

    use super::util::to_utf8_string;

    /// Parse the command line arguments
    ///
    pub fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Args> {
        let args = args.collect::<Vec<_>>();
        ensure!(
            args.len() >= 2,
            "Need at least two  arguments: <wop [source-file]> or <wop [command] [source-file]>"
        );
        ensure!(args[0] == "wop", "First argument must be wop");

        if has_extension(args[1].as_os_str()) {
            let res = DefaultAction::new(&args[1])
                .with_args(args.iter().skip(2))
                .into_args();
            return Ok(res);
        }

        let command = to_utf8_string(&args[1])?;
        let rest_args = &args[2..];

        let result = match command.as_str() {
            "manifest" => {
                ensure!(
                    rest_args.len() == 1,
                    "The manifest command expects the target source file as a single argument",
                );
                let target = PathBuf::from(&rest_args[0]);
                Args::Manifest(target)
            }
            "write-manifest" => {
                ensure!(
                    rest_args.len() == 1,
                    "The manifest command expects the target source file as a single argument",
                );
                let target = PathBuf::from(&rest_args[0]);
                Args::WriteManifest(target)
            }
            "help" | "--help" => {
                ensure!(
                    rest_args.is_empty(),
                    "The help command does not understand extra arguments"
                );
                Args::Help
            }
            "new" => {
                if rest_args.is_empty() {
                    Args::ListTemplates
                } else if rest_args.len() == 2 {
                    let template = rest_args[0]
                        .to_str()
                        .ok_or_else(|| anyhow!("Cannot convert template to utf8 string"))?
                        .to_owned();
                    let target = PathBuf::from(&rest_args[1]);
                    Args::New(template, target)
                } else {
                    bail!(
                        "Invalid new call: either use 'cargo wop new' to list \
                        available templates or 'cargo wop new TEMPLATE PATH' \
                        to create a new file"
                    );
                }
            }
            _ if is_cargo_command(&command) => {
                let target = rest_args
                    .get(0)
                    .ok_or_else(|| anyhow!("Cargo commands require a target source file"))?;
                let rest_args = &rest_args[1..];

                CargoCall::new(command, target)
                    .with_args(rest_args)
                    .normalize()?
                    .into_args()
            }
            _ => bail!(
                "Unknown command: {}. Use 'cargo wop' help to show available commands.",
                command
            ),
        };
        Ok(result)
    }

    #[derive(Debug, PartialEq)]
    pub enum Args {
        /// Execute the default action
        DefaultAction(DefaultAction),
        /// Execute a direct cargo command
        GenericCargoCall(CargoCall),
        /// A build step
        BuildCargoCall(CargoCall),
        /// A install step that gets passed the manifest dir not the file
        InstallCargoCall(CargoCall),
        /// Print out the manifest
        Manifest(PathBuf),
        /// Write the manifest to the current directory
        WriteManifest(PathBuf),
        /// Show usage info and general help
        Help,
        /// Show available templates for new
        ListTemplates,
        /// Create a new file
        New(String, PathBuf),
    }

    #[derive(Debug, PartialEq)]
    pub struct DefaultAction {
        pub target: PathBuf,
        pub args: Vec<OsString>,
    }

    impl DefaultAction {
        pub fn new<Target>(target: Target) -> Self
        where
            Target: Into<PathBuf>,
        {
            DefaultAction {
                target: target.into(),
                args: Vec::new(),
            }
        }

        pub fn with_args<Args, Arg>(mut self, args: Args) -> Self
        where
            Args: IntoIterator<Item = Arg>,
            Arg: Into<OsString>,
        {
            self.args.extend(args.into_iter().map(|arg| arg.into()));
            self
        }

        pub fn into_args(self) -> Args {
            Args::DefaultAction(self)
        }
    }

    #[derive(Debug, PartialEq)]
    pub struct CargoCall {
        pub command: String,
        pub target: PathBuf,
        pub args: Vec<OsString>,
    }

    #[derive(Debug, PartialEq)]
    pub struct Exec {
        pub command: OsString,
        pub target: PathBuf,
        pub args: Vec<OsString>,
    }

    impl CargoCall {
        pub fn new<Command, Target>(command: Command, target: Target) -> Self
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

        pub fn with_args<Args, Arg>(mut self, args: Args) -> Self
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

        pub fn into_args(self) -> Args {
            match self.command.as_str() {
                "build" => Args::BuildCargoCall(self),
                "install" => Args::InstallCargoCall(self),
                _ => Args::GenericCargoCall(self),
            }
        }
    }

    fn has_extension(s: &OsStr) -> bool {
        AsRef::<Path>::as_ref(s).extension().is_some()
    }

    fn is_cargo_command(command: &str) -> bool {
        matches!(
            command,
            "bench"
                | "build"
                | "build-debug"
                | "check"
                | "clean"
                | "clippy"
                | "fmt"
                | "install"
                | "locate-project"
                | "metadata"
                | "pkgid"
                | "run"
                | "run-debug"
                | "tree"
                | "test"
                | "verify-project"
        )
    }
}

mod execution {
    use std::{
        collections::HashMap,
        ffi::{OsStr, OsString},
        fs::{self, File},
        io::{BufRead, BufReader},
        path::{Path, PathBuf},
        process::{Command, Stdio},
    };

    use anyhow::{anyhow, bail, ensure, Context, Result};
    use serde_json::Value as JsonValue;
    use sha1::Sha1;
    use toml::Value;

    use crate::argparse::DefaultAction;

    use super::{
        argparse::{Args, CargoCall},
        execution_env::ExecutionEnv,
        manifest_normalization::normalize_manifest,
        manifest_parsing::parse_manifest,
        util::to_utf8_string,
    };

    /// helper marco to simplify early returns with options
    macro_rules! unwrap_or {
        ($opt:expr, $or:expr) => {
            match $opt {
                Some(value) => value,
                None => $or,
            }
        };
    }

    pub fn execute_args(args: Args, env: &impl ExecutionEnv) -> Result<i32> {
        match &args {
            Args::DefaultAction(call) => {
                let project_info = prepare_manifest_dir(&call.target, env)?;
                let merged_args = merge_default_args(call, &project_info.options.default_action);

                println!(":: cargo {}", format_default_args(&merged_args));
                let args = super::parse_args(merged_args.into_iter())?;
                assert!(
                    !matches!(args, Args::DefaultAction(_)),
                    "Recursion detected in default action"
                );

                execute_args(args, env)
            }
            Args::GenericCargoCall(call) => {
                let project_info = prepare_manifest_dir(&call.target, env)?;
                let exit_code = execute_cargo_call(&call, &project_info)?;
                Ok(exit_code)
            }
            Args::BuildCargoCall(call) => {
                let project_info = prepare_manifest_dir(&call.target, env)?;
                let result = execute_cargo_call(&call, &project_info)?;
                ensure!(
                    result == 0,
                    "Error during build. Cannot copy build artifacts"
                );
                let artifacts = collect_build_artifacts(&call, &project_info)?;
                copy_build_artifacts(artifacts, std::env::current_dir()?, &project_info.options)?;
                Ok(0)
            }
            Args::InstallCargoCall(call) => {
                let project_info = prepare_manifest_dir(&call.target, env)?;
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
                let file =
                    File::open(target.as_path()).context("Error while opening manifest path")?;
                let manifest = parse_manifest(file).context("Error while parsing manifest path")?;
                let manifest = normalize_manifest(manifest, target.as_path(), env)
                    .context("Error during normalizing manifest")?;

                print!("{}", toml::to_string(&manifest)?);
                Ok(0)
            }
            Args::WriteManifest(target) => {
                let env = super::execution_env::LocalEnv::from_env(env);
                let file =
                    File::open(target.as_path()).context("Error while opening manifest path")?;
                let manifest = parse_manifest(file).context("Error while parsing manifest path")?;
                let manifest = normalize_manifest(manifest, target.as_path(), &env)
                    .context("Error during normalizing manifest")?;

                use std::io::Write;
                let mut file = File::create("Cargo.toml")?;
                write!(file, "{}", toml::to_string(&manifest)?)?;

                Ok(0)
            }
            Args::Help => {
                println!("{}", super::text::HELP);
                Ok(0)
            }
            Args::ListTemplates => {
                println!("{}", super::text::HELP_TEMPLATES);
                Ok(0)
            }
            Args::New(template, target) => {
                use std::io::Write;

                let source = render_new_file(template, target)?;

                ensure!(
                    !target.exists(),
                    "Target {} already exists",
                    target.display()
                );

                println!("Write {}", target.display());
                let mut file = File::create(target)?;
                file.write_all(source.as_bytes())?;

                Ok(0)
            }
        }
    }

    fn merge_default_args(
        call: &DefaultAction,
        default_action: &Option<Vec<String>>,
    ) -> Vec<OsString> {
        let mut full_args = Vec::new();
        full_args.push(OsString::from("wop"));

        if let Some(default) = default_action.as_ref() {
            full_args.extend(default.iter().map(OsString::from));
        } else {
            full_args.push(OsString::from("run"));
        };
        full_args.extend(call.args.iter().cloned());
        full_args.insert(2, OsString::from(&call.target));

        full_args
    }

    fn format_default_args(args: &[OsString]) -> String {
        let mut res = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i != 0 {
                res.push(' ');
            }
            res.push_str(arg.to_string_lossy().as_ref());
        }
        res
    }

    /// Create the new file source
    ///
    fn render_new_file(template: &str, target: &Path) -> Result<String> {
        let template = match template {
            "--bin" => super::text::TEMPLATE_BIN,
            "--lib" => super::text::TEMPLATE_LIB,
            "--pymodule" => super::text::TEMPLATE_PYMODULE,
            "--wasm" => super::text::TEMPLATE_WASM,
            _ => bail!("Unknown template '{}'", template),
        };

        let repl = |key: &str| -> Result<String> {
            match key {
                "NAME" => {
                    let res = target
                        .file_stem()
                        .ok_or_else(|| anyhow!("Cannot get file stem"))?
                        .to_str()
                        .ok_or_else(|| anyhow!("Cannot get uf8 name"))?
                        .to_owned();
                    Ok(res)
                }
                _ => bail!("Unknown pattern {}", key),
            }
        };

        let source = super::util::format_dynamic(template, repl)?;
        Ok(source)
    }

    /// Execute a cargo call
    ///
    fn execute_cargo_call(call: &CargoCall, project_info: &ProjectInfo) -> Result<i32> {
        let exit_code = build_cargo_call_with_args::<&str>(call, project_info, &[])
            .status()?
            .code()
            .unwrap_or_default();
        Ok(exit_code)
    }

    fn build_cargo_call_with_args<S: AsRef<OsStr>>(
        call: &CargoCall,
        project_info: &ProjectInfo,
        extra_args: &[S],
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

    /// Prepare the cargo project directory
    ///
    /// This commands writes the manifest and copies the source file. After this
    /// step, cargo calls can be made against this directory.
    ///
    fn prepare_manifest_dir(
        target: impl AsRef<Path>,
        env: &impl ExecutionEnv,
    ) -> Result<ProjectInfo> {
        let target = target.as_ref();
        let manifest_dir = find_project_dir(target, env)?;
        let manifest_path = manifest_dir.join("Cargo.toml");

        // TODO: get the name from the normalized manifest in case the user has overwritten it
        let name = to_utf8_string(
            target
                .file_stem()
                .ok_or_else(|| anyhow!("Could not get name"))?,
        )?;

        let manifest = parse_manifest_file(target)?;
        let options = parse_custom_section(&manifest)?;
        let normed_manifest = normalize_manifest(manifest, target, env)?;

        // perform any faillible operations
        fs::create_dir_all(&manifest_dir)?;
        fs::write(&manifest_path, toml::to_string(&normed_manifest)?)?;

        return Ok(ProjectInfo {
            manifest_path,
            manifest_dir,
            name,
            options,
        });

        fn parse_manifest_file(path: impl AsRef<Path>) -> Result<Value> {
            let file = File::open(path)?;
            parse_manifest(file)
        }
    }

    /// Parse the custom section and retrieve cargo-wop configuration
    ///
    fn parse_custom_section(manifest: &Value) -> Result<ProjectOptions> {
        let mut res = ProjectOptions::default();

        let section = unwrap_or! { manifest.get("cargo-wop"), return Ok(res) };

        if let Some(filter) = section.get("filter") {
            let filter = unwrap_or! { filter.as_table(), bail!("Filter must be table") };
            for (src, dst) in filter {
                let dst = unwrap_or! {
                    dst.as_str(),
                    bail!("Invalid destination for source {}, must be a string", src)
                };
                res.filter.insert(src.to_owned(), dst.to_owned());
            }
        }

        if let Some(default_action) = section.get("default-action") {
            let default_action =
                unwrap_or! { default_action.as_array(), bail!("Default action must be an array") };
            let mut converted_action = Vec::new();
            for item in default_action {
                let item = unwrap_or! {
                    item.as_str(),
                    bail!("Each entry in the default action must be a string")
                };
                converted_action.push(item.to_owned());
            }

            res.default_action = Some(converted_action);
        }

        Ok(res)
    }

    /// Execute a cargo call and collect any generated artifacts
    ///
    fn collect_build_artifacts(
        call: &CargoCall,
        project_info: &ProjectInfo,
    ) -> Result<Vec<String>> {
        let output = build_cargo_call_with_args(call, project_info, &["--message-format", "json"])
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

    fn copy_build_artifacts<I, P, T>(from: I, to: T, options: &ProjectOptions) -> Result<()>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
        T: AsRef<Path>,
    {
        for src in from {
            let src = src.as_ref();

            let src_file_name = unwrap_or! { src.file_name(), bail!("Invalid source filename") };
            let dst_file_name = if let Some(src_file_name) = src_file_name.to_str() {
                if let Some(dst_file_name) = options.filter.get(src_file_name) {
                    OsStr::new(dst_file_name)
                } else {
                    OsStr::new(src_file_name)
                }
            } else {
                src_file_name
            };

            if dst_file_name.is_empty() {
                continue;
            }

            let dst = to.as_ref().join(dst_file_name);
            fs::copy(src, dst)?;
        }
        Ok(())
    }

    struct ProjectInfo {
        name: String,
        manifest_path: PathBuf,
        manifest_dir: PathBuf,
        options: ProjectOptions,
    }

    #[derive(Default, Debug)]
    struct ProjectOptions {
        /// Rename or skip build artifacts
        filter: HashMap<String, String>,
        default_action: Option<Vec<String>>,
    }

    /// Find the project directory from the supplied file
    ///
    fn find_project_dir(source: impl AsRef<Path>, env: &impl ExecutionEnv) -> Result<PathBuf> {
        let source = source.as_ref();
        let source = env.normalize(source)?;

        let target_name = source
            .file_stem()
            .ok_or_else(|| anyhow!("Could not get path stem"))?;
        let mut target_name = target_name.to_owned();
        target_name.push("-");
        target_name.push(&hash_path(source));

        let mut result = find_cache_dir(env)?;
        result.push(target_name);

        Ok(result)
    }

    /// Find the internal cache dir for cargo-wop
    ///
    fn find_cache_dir(env: &impl ExecutionEnv) -> Result<PathBuf> {
        let mut result = env.get_cargo_home_dir();
        result.push("wop-cache");
        Ok(result)
    }

    fn hash_path(path: impl AsRef<Path>) -> String {
        let mut hash = Sha1::new();
        hash.update(path.as_ref().to_string_lossy().as_bytes());
        let digest = hash.digest();
        let res = digest.to_string();
        res[..8].to_string()
    }

    #[cfg(test)]
    mod test {
        use super::*;

        // wrapper to simplify test code
        fn merge_default_args(call: DefaultAction, default_action: Option<&[&str]>) -> Vec<String> {
            let default_action = default_action.map(to_strings);
            super::merge_default_args(&call, &default_action)
                .iter()
                .map(|s| OsString::into_string(s.clone()).unwrap())
                .collect()
        }

        fn to_strings(data: &[&str]) -> Vec<String> {
            data.iter().map(|s| (*s).to_owned()).collect()
        }

        #[test]
        fn test_merge_default_args() {
            assert_eq!(
                merge_default_args(DefaultAction::new("foo.rs"), None),
                to_strings(&["wop", "run", "foo.rs"]),
            );

            assert_eq!(
                merge_default_args(
                    DefaultAction::new("foo.rs").with_args(&["--", "hello", "world"]),
                    None
                ),
                to_strings(&["wop", "run", "foo.rs", "--", "hello", "world"]),
            );

            assert_eq!(
                merge_default_args(
                    DefaultAction::new("foo.rs").with_args(&["test", "--", "hello", "world"]),
                    Some(&[])
                ),
                to_strings(&["wop", "test", "foo.rs", "--", "hello", "world"]),
            );

            assert_eq!(
                merge_default_args(
                    DefaultAction::new("foo.rs"),
                    Some(&["build", "--target", "wasm32-unknown-unknown"])
                ),
                to_strings(&[
                    "wop",
                    "build",
                    "foo.rs",
                    "--target",
                    "wasm32-unknown-unknown"
                ]),
            );
        }
    }
}

mod execution_env {
    use std::path::{Path, PathBuf};

    use anyhow::{bail, Context, Result};

    /// The environment the command is executed in
    ///
    /// It's defined as a trait to mock it out in tests.
    ///
    pub trait ExecutionEnv: Clone {
        fn get_cargo_home_dir(&self) -> PathBuf;
        fn normalize<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf>;
    }

    #[derive(Clone)]
    pub struct StdExecutionEnv {
        working_directory: PathBuf,
        cargo_directory: PathBuf,
    }

    impl StdExecutionEnv {
        pub fn new() -> Result<Self> {
            let this = Self {
                working_directory: std::env::current_dir()?,
                cargo_directory: find_cargo_home_dir()?,
            };
            Ok(this)
        }
    }

    impl ExecutionEnv for StdExecutionEnv {
        fn get_cargo_home_dir(&self) -> PathBuf {
            self.cargo_directory.clone()
        }

        fn normalize<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
            let p = self.working_directory.join(path);
            let p = p
                .canonicalize()
                .with_context(|| format!("Cannot canonicalize {}", p.display()))?;
            Ok(p)
        }
    }

    #[derive(Clone)]
    pub struct LocalEnv {
        cargo_directory: PathBuf,
    }

    impl LocalEnv {
        pub fn from_env(env: &impl ExecutionEnv) -> Self {
            Self {
                cargo_directory: env.get_cargo_home_dir(),
            }
        }
    }

    impl ExecutionEnv for LocalEnv {
        fn get_cargo_home_dir(&self) -> PathBuf {
            self.cargo_directory.clone()
        }

        fn normalize<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
            Ok(path.as_ref().into())
        }
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
}

mod manifest_normalization {
    use std::path::Path;

    use anyhow::{anyhow, bail, ensure, Context, Result};
    use toml::{value::Table, Value};

    use super::{execution_env::ExecutionEnv, util::to_utf8_string};

    /// Normalize the embedded manifest that it can be used to build the target
    ///
    /// This function inserts a package section with name, version, and edition. It
    /// makes sure at least a single target exists (a binary as a default). For each
    /// target it sets the correct path to the source file.
    ///
    pub fn normalize_manifest(
        mut manifest: Value,
        target_path: impl AsRef<Path>,
        env: &impl ExecutionEnv,
    ) -> Result<Value> {
        let target_path = target_path.as_ref();
        let target_directory = target_path
            .parent()
            .ok_or_else(|| anyhow!("Cannot get parent of target path"))?
            .to_owned();

        let target_name = to_utf8_string(
            target_path
                .file_stem()
                .ok_or_else(|| anyhow!("Cannot build manifest for non file target"))?,
        )?;

        let root = manifest
            .as_table_mut()
            .ok_or_else(|| anyhow!("Can only handle manifests that are tables"))?;

        strip_custom_section(root);
        ensure_valid_package(root, &target_name).context("Error while modifying package")?;
        ensure_at_least_a_single_target(root).context("Error while ensuring a valid target")?;

        patch_all_targets(root, target_path, &target_name, env)
            .context("Error while patching the targets")?;
        normalize_paths(root, &target_directory, env)
            .context("Error while normalizing the file paths")?;

        Ok(manifest)
    }

    /// Strip the custom section used to configure cargo-wop
    ///
    fn strip_custom_section(root: &mut toml::map::Map<String, Value>) {
        root.remove("cargo-wop");
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
        path: &Path,
        name: &str,
        env: &impl ExecutionEnv,
    ) -> Result<()> {
        if let Some(lib) = root.get_mut("lib") {
            patch_target(lib, path, &name, env)?;
        }

        if let Some(bins) = root.get_mut("bin") {
            let bins = bins
                .as_array_mut()
                .ok_or_else(|| anyhow!("Invalid manifest: bin not an array"))?;

            for bin in bins {
                patch_target(bin, path, &name, env)?;
            }
        }

        Ok(())
    }

    /// Helper for normalize manifest: patch the target definition to use the correct file path
    fn patch_target(
        target: &mut Value,
        path: &Path,
        name: &str,
        env: &impl ExecutionEnv,
    ) -> Result<()> {
        let path = env.normalize(path)?;
        let path = path
            .to_str()
            .ok_or_else(|| anyhow!("Cannot interpret path as UTF-8 string"))?
            .to_owned();

        let bin = target
            .as_table_mut()
            .ok_or_else(|| anyhow!("Cannot patch non table target"))?;
        bin.insert(String::from("path"), Value::String(path));

        if !bin.contains_key("name") {
            bin.insert(String::from("name"), Value::String(name.to_owned()));
        }

        Ok(())
    }

    /// The key path & mode for normalization
    ///
    /// Use an empty path to denote arrays or arbitrary children. All paths are
    /// interpreted as relative to the project source path.
    ///
    const PATH_NORMALIZATION: &[&[&str]] = &[
        &["lib", "path"],
        &["bin", "", "path"],
        &["example", "", "path"],
        &["test", "", "path"],
        &["bench", "", "path"],
        &["package", "build"],
        &["dependencies", "", "path"],
        &["dev-dependencies", "", "path"],
        &["build-dependencies", "", "path"],
        &["patch", "", "", "path"],
        &["target", "", "dependencies", "", "path"],
    ];

    fn normalize_paths(
        root: &mut toml::map::Map<String, Value>,
        project_source_path: impl AsRef<Path>,
        env: &impl ExecutionEnv,
    ) -> Result<()> {
        let project_source_path = project_source_path.as_ref();
        for path in PATH_NORMALIZATION.iter().copied() {
            let child = match root.get_mut(path[0]) {
                Some(child) => child,
                None => continue,
            };
            _normalize_paths(child, project_source_path, env, path, 1)?;
        }
        Ok(())
    }

    fn _normalize_paths(
        current: &mut Value,
        project_source_path: &Path,
        env: &impl ExecutionEnv,
        path: &[&str],
        depth: usize,
    ) -> Result<()> {
        if depth + 1 == path.len() {
            let current = match current.as_table_mut() {
                Some(current) => current,
                // NOTE: the containing item does not need to be table, e.g.,
                // when the dependency is directly assigned to a version
                None => return Ok(()),
            };

            _normalize_table_item(current, project_source_path, env, path[depth])?;
            return Ok(());
        }
        match current {
            Value::Array(current) => {
                // TODO: improve error message
                ensure!(
                    path[depth].is_empty(),
                    "Unexpected array in path {:?}",
                    &path[..depth + 1]
                );
                for item in current {
                    _normalize_paths(item, project_source_path, env, path, depth + 1)?;
                }
            }
            Value::Table(current) => {
                if path[depth].is_empty() {
                    for (_, item) in current {
                        _normalize_paths(item, project_source_path, env, path, depth + 1)?;
                    }
                } else if current.contains_key(path[depth]) {
                    _normalize_paths(
                        &mut current[path[depth]],
                        project_source_path,
                        env,
                        path,
                        depth + 1,
                    )?;
                }
            }
            // TODO: improve error message
            _ => bail!("Invalid value type"),
        }

        Ok(())
    }

    fn _normalize_table_item(
        current: &mut Table,
        project_source_path: &Path,
        env: &impl ExecutionEnv,
        key: &str,
    ) -> Result<()> {
        if !current.contains_key(key) {
            return Ok(());
        }

        let normed_path = current
            .get(key)
            .unwrap()
            .as_str()
            .ok_or_else(|| anyhow!("Invalid manifest: non string path"))?;
        let normed_path = env.normalize(project_source_path.join(normed_path))?;

        let normed_path = normed_path
            .to_str()
            .ok_or_else(|| anyhow!("Cannot interpret dependency path a string"))?;

        current[key] = normed_path.into();

        Ok(())
    }
}

mod manifest_parsing {
    use std::io::{BufRead, BufReader, Read};

    use anyhow::{bail, Result};
    use toml::Value;

    /// Parse the manifest from the initial doc comment
    ///
    pub fn parse_manifest(reader: impl Read) -> Result<Value> {
        let reader = BufReader::new(reader);

        let mut state = ParseState::Start;
        let mut result = String::new();

        for line in reader.lines() {
            let line = line?;
            let line_start = LineStart::from(line.as_str());

            state = match (state, line_start) {
                (ParseState::Start, LineStart::Other) => {
                    return Ok(Value::Table(Default::default()))
                }
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
                (ParseState::Manifest, LineStart::ManifestEnd) => {
                    return Ok(toml::from_str(&result)?)
                }
                (ParseState::Manifest, LineStart::ManifestStart) => bail!("Invalid manifest"),
                (ParseState::Manifest, LineStart::Other) => bail!("Invalid manifest"),
            };
        }

        if state == ParseState::Manifest {
            bail!("Incomplete manifest");
        }

        Ok(Value::Table(Default::default()))
    }

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

mod util {
    use anyhow::{anyhow, bail, Result};
    use std::ffi::OsStr;

    pub fn to_utf8_string(s: &OsStr) -> Result<String> {
        let result = s
            .to_str()
            .ok_or_else(|| anyhow!("Could not get utf8 rep"))?
            .to_owned();
        Ok(result)
    }

    /// A format-like function that uses a function to lookup replacements
    ///
    pub fn format_dynamic<F>(template: &str, mut replacement: F) -> Result<String>
    where
        F: FnMut(&str) -> Result<String>,
    {
        fn find_from(haystack: &str, needle: char, offset: usize) -> Option<usize> {
            match (&haystack[offset..]).find(needle) {
                Some(res) => Some(res + offset),
                None => None,
            }
        }

        let mut res = String::new();
        let mut offset = 0;

        while let Some(start) = find_from(template, '%', offset) {
            res.push_str(&template[offset..start]);

            let start = start + '%'.len_utf8();
            let end = match find_from(template, '%', start) {
                Some(end) => end,
                None => bail!("Could not find closing '%'"),
            };

            match &template[start..end] {
                // escaped %
                "" => res.push('%'),
                key => {
                    let r = replacement(key)?;
                    res.push_str(&r);
                }
            };

            offset = end + '%'.len_utf8();
        }

        res.push_str(&template[offset..]);
        Ok(res)
    }
}

mod text {
    pub const TEMPLATE_BIN: &str = r##"//! Executable %NAME%
//!
//! ```cargo
//! [dependencies]
//! # include additional dependencies here
//! ```

fn main() {
    println!("Hello world");
}
"##;

    pub const TEMPLATE_LIB: &str = r##"//! Shared library %NAME%
//!
//! This library can be built with `cargo wop`:
//!
//! ```bash
//! cargo wop %NAME%.rs
//! ```
//!
//! ```cargo
//! [lib]
//! name = "%NAME%"
//! crate-type = ["cdylib"]
//!
//! [dependencies]
//! # include additional dependencies here
//!
//! [cargo-wop]
//! default-action = ["build"]
//! ```

#[no_mangle]
pub extern "C" fn add(a: i64, b: i64) -> i64 {
    a + b
}
"##;

    pub const TEMPLATE_PYMODULE: &str = r##"//! Python extension module %NAME%
//!
//! This module can be built with `cargo wop` and imported with Python:
//!
//! ```bash
//! cargo wop %NAME%.rs
//! python -c 'import %NAME%'
//! ```
//!
//! ```cargo
//! [lib]
//! name = "%NAME%"
//! crate-type = ["cdylib"]
//!
//! [dependencies]
//! pyo3 = { version = "0.13", features = ["extension-module"] }
//!
//! [cargo-wop]
//! default-action = ["build"]
//! filter = { "lib%NAME%.so" = "%NAME%.so" }
//! ```
#![allow(unused)]
fn main() {
    use pyo3::prelude::*;
    use pyo3::wrap_pyfunction;

    #[pyfunction]
    fn add(a: i64, b: i64) -> PyResult<i64> {
        Ok(a + b)
    }

    #[pymodule]
    fn %NAME%(py: Python, m: &PyModule) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(add, m)?)?;
        Ok(())
    }
}
"##;

    pub const TEMPLATE_WASM: &str = r##"//! WASM module %NAME%
//!
//! This library can be built with `cargo wop`:
//!
//! ```bash
//! cargo wop %NAME%.rs
//! ```
//!
//! ```cargo
//! [lib]
//! name = "%NAME%"
//! crate-type = ["cdylib"]
//!
//! [dependencies]
//! # include additional dependencies here
//!
//! [cargo-wop]
//! default-action = ["build", "--target", "wasm32-unknown-unknown"]
//! ```

#[no_mangle]
pub extern "C" fn add(a: i64, b: i64) -> i64 {
    a + b
}
"##;

    pub const HELP: &str = r##"cargo wop -- cargo without project

Run the rust source file as a script:

    cargo wop SOURCE.rs
    cargo wop run SOURCE.rs
    cargo wop run SOURCE.rs [SCRIPT ARGUMENTS ...]
    cargo wop run SOURCE.rs [CARGO ARGUMENTS ...] -- [SCRIPT ARGUMENTS ...]

Build the included targets, executables or libraries:

    cargo wop build SOURCE.rs [CARGO ARGUMENTS ...]

Per default run and build use release builds. Use the run-debug / build-debug
commands for debug builds.

cargo wop supports the following cargo commands:

    bench check clean clippy fmt install locate-project metadata pkgid tree
    test verify-project

They can be executed as

    cargo wop COMMAND SOURCE.rs [CARGO ARGUMENTS ...]

In addition the following extra commands are supported:

    cargo wop manifest SOURCE.rs        - Show the generated manifest file
    cargo wop write-manifest SOURCE.rs  - Write the generated manifest to the
                                          current directory as Cargo.toml
    cargo wop new                       - List available templates to create
                                          a new file
    cargo wop new TEMPLATE SOURCE.rs    - Create the file SOURCE.rs using the
                                          given template
    cargo wop help                      - Show this help text
    cargo wop --help
"##;

    pub const HELP_TEMPLATES: &str = r##"The following templates are available:

- "--bin": an executable
- "--lib": a library
- "--pymodule": a library using PyO3 that compiles to a Python extension module
- "--wasm": a standalone wasm32 module
"##;
}

#[cfg(test)]
mod test_parse_args {
    use super::argparse::{Args, CargoCall, DefaultAction};
    use anyhow::Result;
    use std::{ffi::OsString, path::PathBuf};

    /// Helper to simplify using parse_args
    fn parse_args(args: &[&str]) -> Result<Args> {
        let mut os_args = Vec::<OsString>::new();
        for arg in args {
            os_args.push(OsString::from(*arg));
        }

        super::parse_args(os_args.into_iter())
    }

    /// Test parsing run commands
    #[test]
    fn example_implicit_run() {
        assert_eq!(
            parse_args(&["wop", "example.rs"]).unwrap(),
            DefaultAction::new("example.rs").into_args(),
        );
    }

    /// Test parsing run-debug commands
    #[test]
    fn example_run_debug() {
        assert_eq!(
            parse_args(&["wop", "run-debug", "example.rs"]).unwrap(),
            CargoCall::new("run", "example.rs").into_args(),
        );
    }

    /// Test parsing build commands
    #[test]
    fn example2() {
        let actual = parse_args(&["wop", "build", "example.rs"]).unwrap();
        let expected = CargoCall::new("build", "example.rs")
            .with_args(&["--release"])
            .into_args();

        assert_eq!(actual, expected);
    }

    /// Test parsing run commands with additional arguments for cargo
    #[test]
    fn cargo_args() {
        let actual = parse_args(&["wop", "run", "example.rs", "--verbose", "--", "arg"]).unwrap();
        let expected = CargoCall::new("run", "example.rs")
            .with_args(&["--verbose", "--release", "--", "arg"])
            .into_args();

        assert_eq!(actual, expected);
    }

    /// Test parsing run-debug commands with additional arguments for cargo
    #[test]
    fn cargo_args_debug() {
        let actual =
            parse_args(&["wop", "run-debug", "example.rs", "--verbose", "--", "arg"]).unwrap();
        let expected = CargoCall::new("run", "example.rs")
            .with_args(&["--verbose", "--", "arg"])
            .into_args();

        assert_eq!(actual, expected);
    }

    /// Test parsing manifest commands
    #[test]
    fn manifest_example() {
        let actual = parse_args(&["wop", "manifest", "example.rs"]).unwrap();
        let expected = Args::Manifest(PathBuf::from("example.rs"));

        assert_eq!(actual, expected);
    }

    /// Test that manifest commands with more than one argument are rejected
    #[test]
    fn manifest_example_error() {
        let actual = parse_args(&["wop", "manifest", "example.rs", "second-arg"]);
        assert!(actual.is_err());
    }
}

#[cfg(test)]
mod test_parse_manifest {
    use super::manifest_parsing::parse_manifest;
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

#[cfg(test)]
mod test_rust_path_handling {
    use std::path::PathBuf;

    fn parent_path(p: &str) -> Option<PathBuf> {
        PathBuf::from(p).parent().map(|p| p.to_owned())
    }

    #[test]
    fn examples() {
        assert_eq!(parent_path("example.rs"), Some(PathBuf::from("")));
        assert_eq!(parent_path("./example.rs"), Some(PathBuf::from(".")));
        assert_eq!(parent_path("foo/example.rs"), Some(PathBuf::from("foo")));
        assert_eq!(
            parent_path("foo/bar/example.rs"),
            Some(PathBuf::from("foo/bar"))
        );
    }
}

#[cfg(test)]
mod test_format_dynamic {
    use super::util::format_dynamic;
    use anyhow::Result;

    #[test]
    fn examples() -> Result<()> {
        let mut repl = |s: &str| -> Result<String> {
            match s {
                "hello" => Ok(String::from("world")),
                "foo" => Ok(String::from("bar")),
                _ => Ok(String::from(s)),
            }
        };

        assert_eq!(format_dynamic("%foo%", &mut repl)?, String::from("bar"));
        assert_eq!(
            format_dynamic("%foo% %hello%", &mut repl)?,
            String::from("bar world")
        );
        assert_eq!(
            format_dynamic("leading %foo% %hello% trailing", &mut repl)?,
            String::from("leading bar world trailing")
        );
        assert_eq!(
            format_dynamic(".. %foo% .. %foo% .. %foo% ..", &mut repl)?,
            String::from(".. bar .. bar .. bar ..")
        );

        assert_eq!(
            format_dynamic(".. %%foo%% ..", &mut repl)?,
            String::from(".. %foo% .."),
        );

        Ok(())
    }
}
