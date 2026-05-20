// Example code that deserializes and serializes the model.
// extern crate serde;
// #[macro_use]
// extern crate serde_derive;
// extern crate serde_json;
//
// use generated_module::Welcome;
//
// fn main() {
//     let json = r#"{"answer": 42}"#;
//     let model: Welcome = serde_json::from_str(&json).unwrap();
// }

use core::fmt::Display;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Opcodes {
    pub unprefixed: HashMap<String, Opcode>,

    pub cbprefixed: HashMap<String, Opcode>,
}

impl Opcodes {}

impl Default for Opcodes {
    fn default() -> Self {
        serde_json::from_str(include_str!("opcodes.json")).unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Opcode {
    mnemonic: String,

    bytes: i64,

    cycles: Vec<i64>,

    operands: Vec<Operand>,

    immediate: bool,

    flags: Flags,
}
impl Display for Opcode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Opcode {
            operands, mnemonic, ..
        } = self;
        let operands = operands
            .iter()
            .map(|operand| operand.name.clone())
            .join(" ");

        write!(f, "{mnemonic} {operands}")
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Flags {
    z: String,

    n: String,

    h: String,

    c: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Operand {
    name: String,

    immediate: bool,

    bytes: Option<i64>,

    increment: Option<bool>,

    decrement: Option<bool>,
}
