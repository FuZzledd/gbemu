use core::error::Error;
use std::{collections::BTreeMap, fs};

use databake::*;
use octopus_common::theme::*;
use glob::glob;
use vergen_gix::{Build, Cargo, Emitter, Gix, Rustc, Sysinfo};

fn main() -> Result<(), Box<dyn Error>> {
    let themes = glob(concat!(env!("CARGO_MANIFEST_DIR"), "/themes/**/*.yaml"))
        .expect("Couldn't read glob")
        .filter_map(|path| path.ok())
        .map(|file| yaml_serde::from_str::<Theme>(&fs::read_to_string(file).unwrap()).unwrap())
        .map(|theme| (theme.name.clone(), theme))
        .collect::<BTreeMap<_, _>>();

    fs::write(
        format!("{}/themes_generated", std::env::var("OUT_DIR")?),
        themes.bake(&Default::default()).to_string(),
    )?;

    let build = Build::all_build();
    let cargo = Cargo::all_cargo();
    let git = Gix::all().sha(true).build();
    let rustc = Rustc::all_rustc();
    let si = Sysinfo::all_sysinfo();

    Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&git)?
        .add_instructions(&rustc)?
        .add_instructions(&si)?
        .emit()?;

    println!("cargo::rerun-if-changed=themes");

    Ok(())
}
