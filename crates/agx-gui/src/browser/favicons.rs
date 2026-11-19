//! Site icons for the bookmark bar. `scripts/extract-favicons.py` copies them out of a local Chrome
//! profile into `~/.config/agx/favicons/<host>.png`; nothing here touches the network.
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::{collections::HashMap, fs, path::PathBuf};

const MAX_FILE: u64 = 64 * 1024;
const MAX_SIDE: u32 = 128;

#[derive(Default)]
pub(super) struct Icons {
    dir: Option<PathBuf>,
    cache: HashMap<String, Option<Image>>,
}

impl Icons {
    pub fn new() -> Self {
        Self { dir: super::bookmarks::config_dir().map(|dir| dir.join("favicons")), cache: HashMap::new() }
    }

    /// The icon for a host (without `www.`), decoded once and remembered, hit or miss.
    pub fn get(&mut self, host: &str) -> Option<Image> {
        if let Some(found) = self.cache.get(host) {
            return found.clone();
        }
        let image = self.load(host);
        self.cache.insert(host.to_string(), image.clone());
        image
    }

    fn load(&self, host: &str) -> Option<Image> {
        let safe = !host.is_empty() && host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-');
        let path = self.dir.as_ref()?.join(format!("{host}.png"));
        if !safe || fs::metadata(&path).ok()?.len() > MAX_FILE {
            return None;
        }
        decode(&fs::read(path).ok()?)
    }
}

fn decode(bytes: &[u8]) -> Option<Image> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    if info.width == 0 || info.height == 0 || info.width > MAX_SIDE || info.height > MAX_SIDE {
        return None;
    }
    let mut data = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut data).ok()?;
    let data = &data[..frame.buffer_size()];
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(frame.width, frame.height);
    let out = pixels.make_mut_slice();
    let channels = match frame.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    for (pixel, src) in out.iter_mut().zip(data.chunks_exact(channels)) {
        *pixel = match channels {
            4 => Rgba8Pixel::new(src[0], src[1], src[2], src[3]),
            3 => Rgba8Pixel::new(src[0], src[1], src[2], 255),
            2 => Rgba8Pixel::new(src[0], src[0], src[0], src[1]),
            _ => Rgba8Pixel::new(src[0], src[0], src[0], 255),
        };
    }
    Some(Image::from_rgba8(pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_small_png_and_rejects_garbage() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255, 0, 0, 255, 0, 255, 0, 128]).unwrap();
        }
        let image = decode(&bytes).unwrap();
        assert_eq!((image.size().width, image.size().height), (2, 1));
        assert!(decode(b"not a png").is_none());
    }
}
