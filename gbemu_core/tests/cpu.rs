use std::{fs, path::Path};

use datatest_stable::Utf8Path;
use gbemu_core::{
    context::{Context, FlatMemory, Memory},
    cpu::CPU,
    opcode::Opcodes,
};

datatest_stable::harness! {
    {test = test_opcode, root = "test_fixtures/sm83/v1", pattern = r".*\.json"}
}

use serde::{Deserialize, Serialize};

type OpcodeTests = Vec<OpcodeTest>;

#[derive(Debug, Serialize, Deserialize)]
pub struct OpcodeTest {
    name: String,

    #[serde(rename = "initial")]
    initial_state: State,

    #[serde(rename = "final")]
    final_state: State,

    cycles: Vec<(u16, Option<u8>, String)>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    pc: u16,

    sp: u16,

    a: u8,

    b: u8,

    c: u8,

    d: u8,

    e: u8,

    f: u8,

    h: u8,

    l: u8,

    ime: u8,

    #[serde(default)]
    ei: Option<u8>,

    ram: Vec<(u16, u8)>,
}

fn test_opcode(path: &Utf8Path, content: String) -> datatest_stable::Result<()> {
    let opcodes = Opcodes::default();

    let test_data: OpcodeTests = serde_json::from_str(&content)?;

    for (index, opcode_test) in test_data.into_iter().enumerate() {
        println!("\nTest {index}");
        let mut cpu = CPU::<FlatMemory>::default();
        let mut context = Context::<FlatMemory>::default();
        let OpcodeTest {
            initial_state,
            final_state,
            cycles,
            ..
        } = opcode_test;
        {
            let State {
                pc,
                sp,
                a,
                b,
                c,
                d,
                e,
                f,
                h,
                l,
                ime,
                ei,
                ram,
            } = initial_state;
            cpu.pc = pc;
            cpu.registers.sp = sp;
            cpu.registers.a = a;
            cpu.registers.b = b;
            cpu.registers.c = c;
            cpu.registers.d = d;
            cpu.registers.e = e;
            *cpu.registers.f = f;
            cpu.registers.h = h;
            cpu.registers.l = l;

            for (address, value) in ram {
                println!("Setting address {address:04X} to {value:02X}");
                context.memory.write_u8(address, value);
            }
        }
        cpu.state = gbemu_core::cpu::State::Decode(0);
        println!("{}", cpu.dump_state(&mut context));
        cpu.ir = context.memory.read_u8(cpu.pc);
        cpu.increment_pc(&mut context);
        let mut cb = false;
        for cycle in cycles {
            println!(
                "Current instruction: PC: 0x{:04X} - 0x{:02X} ({})",
                cpu.pc.wrapping_sub(1),
                cpu.ir,
                (if cb {
                    &opcodes.cbprefixed
                } else {
                    &opcodes.unprefixed
                })[&format!("0x{:02X}", cpu.ir)]
            );
            cb = cpu.ir == 0xCB;

            cpu.tick(&mut context);
            if cpu.halted {
                println!("CPU HALTED");
            }
            println!("CPU State: {:02X?}", cpu.state);
            println!("{}", cpu.dump_state(&mut context));
        }
        {
            let State {
                pc,
                sp,
                a,
                b,
                c,
                d,
                e,
                f,
                h,
                l,
                ime,
                ei,
                ram,
            } = final_state;
            if !cpu.halted {
                assert_eq!(
                    cpu.pc.wrapping_sub(1),
                    pc,
                    "PC {:04X} == {:04X}",
                    cpu.pc.wrapping_sub(1),
                    pc,
                );
            } else {
                assert_eq!(cpu.pc, pc, "PC {:04X} == {:04X}", cpu.pc, pc,);
            }

            assert_eq!(
                cpu.registers.sp, sp,
                "SP {:04X} == {:04X}",
                cpu.registers.sp, sp,
            );
            assert_eq!(cpu.registers.a, a, "A {:02X} == {:02X}", cpu.registers.a, a);
            assert_eq!(cpu.registers.b, b, "B {:02X} == {:02X}", cpu.registers.b, b);
            assert_eq!(cpu.registers.c, c, "C {:02X} == {:02X}", cpu.registers.c, c);
            assert_eq!(cpu.registers.d, d, "D {:02X} == {:02X}", cpu.registers.d, d);
            assert_eq!(cpu.registers.e, e, "E {:02X} == {:02X}", cpu.registers.e, e);
            assert_eq!(
                *cpu.registers.f, f,
                "F {:02X} == {:02X}",
                *cpu.registers.f, f
            );
            assert_eq!(cpu.registers.h, h, "H {:02X} == {:02X}", cpu.registers.h, h);
            assert_eq!(cpu.registers.l, l, "L {:02X} == {:02X}", cpu.registers.l, l);

            for (address, value) in ram {
                assert_eq!(
                    context.memory.read_u8(address),
                    value,
                    "Address 0x{address:04X}"
                );
            }
        }
    }

    Ok(())
}
