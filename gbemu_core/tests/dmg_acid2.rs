#![feature(hash_map_macro)]

use gbemu_core::{GameBoy, ppu};
use image::ImageFormat::Png;
use rgb::Gray;
use std::path::Path;
use tap::Tap;

mod common;

#[test]

fn test_dmg_acid2() {
    let mut gameboy: GameBoy = GameBoy::default();

    gameboy.palette.tap_mut(|palette| {
        use ppu::Pixel::*;
        palette[White] = Gray::new(0xFF).into();
        palette[LightGray] = Gray::new(0xAA).into();
        palette[DarkGrey] = Gray::new(0x55).into();
        palette[Black] = Gray::new(0x00).into();
    });

    gameboy.load_rom(Path::new("../../test_roms/dmg_acid2/dmg-acid2.gb"));

    for _ in 0..=10 {
        loop {
            if gameboy.tick(false) {
                break;
            }
        }
    }

    let buffer = gameboy.buffer;
    let reference_image = image::load_from_memory_with_format(
        include_bytes!("test_reference_images/dmg-acid2-reference-dmg.png"),
        Png,
    )
    .unwrap()
    .into_rgba8();

    let inline_image = common::inline_iterm2_image_from_buffer(buffer.clone(), "dmg_acid2");
    println!("Result:\n {inline_image}");
    if buffer != *reference_image {
        panic!("Did not match reference image");
    }
}
