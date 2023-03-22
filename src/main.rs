//! # bloggo
//!
//! A command line wrapper around the [bloggo] static site generator library.
use clap::{arg, command};
use log::error;
use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let matches = command!()
        .args(&[
            arg!(-s --source <DIR> "Directory containing post and template source (default: source)"),
            arg!(-o --dest <DIR> "Directory where output will be stored (default: build)"),
            arg!(-b --base <URL> "The base URL for relative links"),
            arg!(-v --verbose "Provide verbose output"),
        ])
        .subcommand_required(true)
        .subcommand(command!("clean").about("Clean destination directory"))
        .subcommand(command!("build").about("Build static site pages"))
        .get_matches();

    let src_dir = arg_or_env_or_default(matches.get_one("source"), "BLOGGO_SRC", "source");
    let dest_dir = arg_or_env_or_default(matches.get_one("dest"), "BLOGGO_DEST", "dest");
    let base_url = arg_or_env_or_default(matches.get_one("base"), "BLOGGO_BASE", "");
    let verbose = matches.get_flag("verbose");

    init_logger(verbose);

    let mut b = bloggo::Builder::new()
        .src_dir(src_dir)
        .dest_dir(dest_dir)
        .base_url(base_url)
        .build();

    let result = match matches.subcommand() {
        Some(("clean", _)) => b.clean(),
        Some(("build", _)) => b.build(),
        _ => panic!("This should never happen."),
    };

    if let Err(e) = result {
        error!("{}", e);
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Get a configuration value using the following steps:
/// 1. If the provided argument value is Some, use it.
/// 2. If the environement variable exists, use it.
/// 3. Otherwise, return the default value.
fn arg_or_env_or_default(arg: Option<&String>, env_var: &str, default: &str) -> String {
    arg.map(|s| s.to_owned())
        .or(env::var(env_var).ok())
        .unwrap_or_else(|| String::from(default))
}

use env_logger::{Builder, Env};

fn init_logger(verbose: bool) {
    let filter_env = "BLOGGO_LOG";
    let write_style_env = "BLOGGO_LOG_STYLE";

    let env = Env::new().filter(filter_env).write_style(write_style_env);
    let mut env_builder = Builder::from_env(env);
    if verbose && env::var_os(filter_env).is_none() {
        env_builder.filter_level(log::LevelFilter::Info);
    }
    env_builder.init();
}
