//! # bloggo
//!
//! A command line wrapper around the [bloggo] static site generator library.
use clap::{arg, command};
use log::error;
use std::process::ExitCode;

fn main() -> ExitCode {
    let matches = command!()
        .args(&[
            arg!(-s --source <DIR> "Directory containing post and template source")
                .default_value("source/"),
            arg!(-o --dest <DIR> "Directory where output will be stored").default_value("build/"),
            arg!(-v --verbose "Provide verbose output"),
        ])
        .subcommand_required(true)
        .subcommand(command!("clean").about("Clean destination directory"))
        .subcommand(command!("build").about("Build static site pages"))
        .get_matches();

    let src_dir = matches.get_one::<String>("source").unwrap();
    let dest_dir = matches.get_one::<String>("dest").unwrap();
    let verbose = matches.get_flag("verbose");

    init_logger(verbose);

    let mut b = bloggo::Builder::new()
        .src_dir(src_dir)
        .dest_dir(dest_dir)
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

use env_logger::{Builder, Env};
use std::env;

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
