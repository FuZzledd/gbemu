use datatest_stable::Utf8Path;
use gbemu_core::GameBoy;
use ringbuf::traits::Consumer;

mod common;

datatest_stable::harness! {
    {test = test_cpu_instrs, root = "../test_roms/blargg_tests/cpu_instrs/individual", pattern = r".*\.gb"}
}

fn test_cpu_instrs(_path: &Utf8Path, rom: Vec<u8>) -> datatest_stable::Result<()> {
    let mut gameboy: GameBoy = GameBoy::default();
    gameboy.cpu.load_rom(&rom, &mut gameboy.context);

    let mut output = Vec::new();

    let status = loop {
        loop {
            let redraw = gameboy.tick(false);
            gameboy
                .context
                .memory
                .io
                .serial
                .output
                .write_into(&mut output, None);
            if redraw {
                break;
            }
        }
        let output = String::try_from(output.clone()).unwrap();
        if output.contains("Passed") {
            break true;
        } else if output.contains("Failed") {
            break false;
        }
    };

    for _ in 0..4 {
        loop {
            let redraw = gameboy.tick(false);
            gameboy
                .context
                .memory
                .io
                .serial
                .output
                .write_into(&mut output, None);
            if redraw {
                break;
            }
        }
    }

    let image = common::inline_iterm2_image_from_buffer(gameboy.buffer.clone());

    println!("Result\n {image}");

    assert!(status);

    Ok(())
}
