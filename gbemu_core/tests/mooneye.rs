use datatest_stable::Utf8Path;
use gbemu_core::{GameBoy, cpu::registers::Registers};

mod common;

datatest_stable::harness! {
    {test = test_acceptance, root = "../test_roms/mooneye-test-suite/build/acceptance", pattern = r".*\.gb"},
    {test = test_emulator_only, root = "../test_roms/mooneye-test-suite/build/emulator-only", pattern = r".*\.gb"}

}

fn test_emulator_only(path: &Utf8Path, rom: Vec<u8>) -> datatest_stable::Result<()> {
    test_acceptance(path, rom)
}

fn test_acceptance(path: &Utf8Path, _rom: Vec<u8>) -> datatest_stable::Result<()> {
    let mut gameboy: GameBoy = GameBoy::default();
    gameboy.load_rom(path);

    let status = loop {
        loop {
            let redraw = gameboy.tick(false);
            if redraw {
                break;
            }
        }
        if let Registers {
            b: 3,
            c: 5,
            d: 8,
            e: 13,
            h: 21,
            l: 34,
            ..
        } = gameboy.cpu.registers
        {
            break true;
        } else if let Registers {
            b: 0x42,
            c: 0x42,
            d: 0x42,
            e: 0x42,
            h: 0x42,
            l: 0x42,
            ..
        } = gameboy.cpu.registers
        {
            break false;
        }
    };

    for _ in 0..4 {
        loop {
            let redraw = gameboy.tick(false);
            if redraw {
                break;
            }
        }
    }

    let image =
        common::inline_iterm2_image_from_buffer(gameboy.buffer.clone(), path.file_name().unwrap());

    println!("Result\n {image}");

    assert!(status);

    Ok(())
}
