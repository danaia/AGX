//! Single-screen RFB desktop resizing: bounded requests and ExtendedDesktopSize replies.
use super::rfb;
use std::io::{self, Read};

pub const ENCODING: i32 = -308;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub width: u16,
    pub height: u16,
    screens: Vec<[u8; 16]>,
}

pub fn supported_size(width: u16, height: u16) -> bool {
    (320..=4096).contains(&width) && (200..=2160).contains(&height)
}

impl Request {
    /// Only a single screen filling the desktop is supported, with stable ID 1.
    /// Client metadata is normalised because LibVNCClient 0.9.15 leaves it uninitialised.
    pub fn valid(&self) -> bool {
        if !supported_size(self.width, self.height) || self.screens.len() != 1 {
            return false;
        }
        self.screens[0][8..12] == screen(self.width, self.height)[8..12]
    }

    pub fn read(reader: &mut impl Read) -> io::Result<Self> {
        let mut header = [0; 7];
        reader.read_exact(&mut header)?;
        let mut screens = vec![[0; 16]; usize::from(header[5])];
        for screen in &mut screens {
            reader.read_exact(screen)?;
        }
        Ok(Self {
            width: u16::from_be_bytes([header[1], header[2]]),
            height: u16::from_be_bytes([header[3], header[4]]),
            screens,
        })
    }
}

fn screen(width: u16, height: u16) -> [u8; 16] {
    let mut screen = [0; 16];
    // LibVNCClient 0.9.15 ignores screen ID zero, although the protocol allows it.
    screen[3] = 1;
    screen[8..10].copy_from_slice(&width.to_be_bytes());
    screen[10..12].copy_from_slice(&height.to_be_bytes());
    screen
}

#[derive(Clone, Debug)]
pub struct Reply {
    pub reason: u16,
    pub status: u16,
    pub width: u16,
    pub height: u16,
}

impl Reply {
    /// ExtendedDesktopSize is a separate update: it must not share one with pixels.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(36);
        rfb::begin_update(&mut out, 1);
        rfb::rectangle_header(
            &mut out,
            super::frame::Rect {
                x: usize::from(self.reason),
                y: usize::from(self.status),
                w: usize::from(self.width),
                h: usize::from(self.height),
            },
            ENCODING,
        );
        out.extend_from_slice(&[1, 0, 0, 0]);
        out.extend_from_slice(&screen(self.width, self.height));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(width: u16, height: u16) -> Vec<u8> {
        let mut out = vec![0];
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&[1, 0]);
        out.extend_from_slice(&screen(width, height));
        out
    }

    #[test]
    fn bounded_single_screen_requests_are_accepted() {
        for (w, h) in [(320, 200), (1920, 1080), (4096, 2160)] {
            assert!(
                Request::read(&mut request(w, h).as_slice())
                    .unwrap()
                    .valid()
            );
        }
        for (w, h) in [(0, 720), (319, 200), (1920, 2161), (65535, 65535)] {
            assert!(
                !Request::read(&mut request(w, h).as_slice())
                    .unwrap()
                    .valid()
            );
        }
        let mut bytes = request(1280, 720);
        bytes[15] = 0; // screen width no longer matches the framebuffer
        assert!(!Request::read(&mut bytes.as_slice()).unwrap().valid());
        assert!(Request::read(&mut &bytes[..10]).is_err());
        let mut legacy = request(1280, 720);
        legacy[7..11].fill(0); // LibVNCClient's missing screen ID
        legacy[19..23].copy_from_slice(&[0x98, 0x55, 0, 0]); // uninitialised flags
        assert!(Request::read(&mut legacy.as_slice()).unwrap().valid());
    }

    #[test]
    fn reply_contains_reason_status_and_single_screen() {
        let out = Reply {
            reason: 1,
            status: 0,
            width: 1920,
            height: 1080,
        }
        .encode();
        assert_eq!(&out[..12], &[0, 0, 0, 1, 0, 1, 0, 0, 7, 128, 4, 56]);
        assert_eq!(&out[12..16], &ENCODING.to_be_bytes());
        assert_eq!(&out[16..20], &[1, 0, 0, 0]);
        assert_eq!(&out[20..], &screen(1920, 1080));
    }
}
