use core::error::Error;
use std::{collections::BTreeMap, fs};

use databake::*;
use gbemu_common::theme::*;
use glob::glob;
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

    println!("cargo::rerun-if-changed=themes");

    Ok(())
}
