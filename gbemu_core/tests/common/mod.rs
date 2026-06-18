use std::io::Cursor;

use bytes::BytesMut;
use image::codecs::png::PngEncoder;

pub fn inline_iterm2_image_from_buffer(buffer: BytesMut) -> String {
    let image = image::RgbaImage::from_vec(160, 144, buffer.to_vec()).unwrap();

    let image_result = Vec::new();
    let mut cursor = Cursor::new(image_result);

    let encoder = PngEncoder::new(&mut cursor);
    image.write_with_encoder(encoder).unwrap();

    iterm2img::from_bytes(cursor.into_inner())
        .name("dmg_acid2 result image".to_string())
        .width(60)
        .height(60)
        .preserve_aspect_ratio(true)
        .inline(true)
        .build()
}
