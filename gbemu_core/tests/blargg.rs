use core::time::Duration;
use std::time::Instant;

use bytes::BytesMut;
use datatest_stable::Utf8Path;
use gbemu_core::{GameBoy, Palette, ppu};
use rgb::Gray;
use ringbuf::traits::Consumer;
use tap::{Conv, Tap};

mod common;

datatest_stable::harness! {
    {test = test_cpu_instrs, root = "../test_roms/blargg_tests/cpu_instrs/individual", pattern = r".*\.gb"}
}

fn test_cpu_instrs(path: &Utf8Path, _rom: Vec<u8>) -> datatest_stable::Result<()> {
    let mut gameboy: GameBoy = GameBoy::default();
    gameboy.reset(true);
    gameboy.load_rom(path).unwrap();

    let mut output = Vec::new();

    let start_time = Instant::now();

    let status = loop {
        gameboy.tick(true);
        gameboy
            .context
            .memory
            .io
            .serial
            .output
            .write_into(&mut output, None);

        let output = String::try_from(output.clone()).unwrap();
        if output.contains("Passed") {
            break true;
        } else if output.contains("Failed") {
            break false;
        }

        if start_time.elapsed() > Duration::from_secs(30) {
            break false;
        }
    };

    let start_time = Instant::now();
    'outer: for _ in 0..4 {
        loop {
            let redraw = gameboy.tick(true).should_redraw();
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
            if start_time.elapsed() > Duration::from_secs(1) {
                break 'outer;
            }
        }
    }

    let palette = Palette::default();
    let buffer: BytesMut = gameboy
        .get_screen()
        .iter()
        .flat_map(|pixel| palette[pixel].conv::<[u8; 4]>())
        .collect();

    let image = common::inline_iterm2_image_from_buffer(buffer, path.file_name().unwrap());

    println!("Result\n {image}",);

    assert!(
        status,
        "Test failed, serial output: {:?}",
        String::try_from(output)
    );

    Ok(())
}
