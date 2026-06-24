use core::fmt::Display;
use std::io::Cursor;

use base64::{display::Base64Display, engine::general_purpose::STANDARD};
use bytes::BytesMut;
use image::codecs::png::PngEncoder;

pub fn inline_iterm2_image_from_buffer(buffer: BytesMut, file_name: impl Display) -> String {
    let image = image::RgbaImage::from_vec(160, 144, buffer.to_vec()).unwrap();

    let image_result = Vec::new();
    let mut cursor = Cursor::new(image_result);

    let encoder = PngEncoder::new(&mut cursor);
    image.write_with_encoder(encoder).unwrap();

    let base64 = Base64Display::new(cursor.get_ref(), &STANDARD);

    let inline = iterm2img::from_bytes(cursor.get_ref().clone())
        .name(format!("{} result image", file_name))
        .width(60)
        .height(60)
        .preserve_aspect_ratio(true)
        .inline(true)
        .build();

    format!("base64: {base64}\n{inline}")
}
