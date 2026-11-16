//! RFB 3.3/3.7/3.8 wire protocol for the server side, with Raw and desktop-size
//! extensions. Pure: it reads and writes byte streams and knows nothing about
//! Slint, threads or sockets.
use super::frame::{Frame, Rect};
use std::io::{self, Read, Write};

pub const ENCODING_RAW: i32 = 0;
pub const ENCODING_DESKTOP_SIZE: i32 = -223;
const MAX_ENCODINGS: usize = 1024;
const MAX_CUT_TEXT: usize = 1 << 20;
const SECURITY_NONE: u8 = 1;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// A true-colour pixel layout chosen by the client (`SetPixelFormat`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelFormat {
    pub bits_per_pixel: u8,
    pub depth: u8,
    pub big_endian: bool,
    /// Maximum value of the red, green and blue channels.
    pub max: [u16; 3],
    /// Bit position of the red, green and blue channels.
    pub shift: [u8; 3],
}

impl PixelFormat {
    /// 32-bit little-endian xRGB, what nearly every client asks for.
    pub const NATIVE: Self = Self {
        bits_per_pixel: 32,
        depth: 24,
        big_endian: false,
        max: [255; 3],
        shift: [16, 8, 0],
    };

    pub fn parse(bytes: &[u8; 16]) -> io::Result<Self> {
        let format = Self {
            bits_per_pixel: bytes[0],
            depth: bytes[1],
            big_endian: bytes[2] != 0,
            max: [
                u16::from_be_bytes([bytes[4], bytes[5]]),
                u16::from_be_bytes([bytes[6], bytes[7]]),
                u16::from_be_bytes([bytes[8], bytes[9]]),
            ],
            shift: [bytes[10], bytes[11], bytes[12]],
        };
        if !matches!(format.bits_per_pixel, 8 | 16 | 32) {
            return Err(invalid("unsupported bits per pixel"));
        }
        if bytes[3] == 0 {
            return Err(invalid("colour-map pixel formats are not supported"));
        }
        for channel in 0..3 {
            let (max, shift) = (
                u64::from(format.max[channel]),
                u32::from(format.shift[channel]),
            );
            if max == 0 || shift >= 32 || (max << shift) >= 1 << format.bits_per_pixel {
                return Err(invalid("pixel format channel does not fit the pixel"));
            }
        }
        Ok(format)
    }

    pub fn to_bytes(self) -> [u8; 16] {
        let mut out = [0; 16];
        out[0] = self.bits_per_pixel;
        out[1] = self.depth;
        out[2] = u8::from(self.big_endian);
        out[3] = 1; // true colour
        for channel in 0..3 {
            out[4 + channel * 2..6 + channel * 2].copy_from_slice(&self.max[channel].to_be_bytes());
            out[10 + channel] = self.shift[channel];
        }
        out
    }

    /// Stream Raw pixels one row at a time, without allocating a whole encoded frame.
    pub fn write_pixels<W: Write>(&self, frame: &Frame, rect: Rect, out: &mut W) -> io::Result<()> {
        let mut row = Vec::with_capacity(rect.w * usize::from(self.bits_per_pixel / 8));
        for y in rect.y..rect.y + rect.h {
            row.clear();
            self.encode(frame, Rect { y, h: 1, ..rect }, &mut row);
            out.write_all(&row)?;
        }
        Ok(())
    }

    /// Append `rect` of `frame` to `out` in this pixel format (Raw encoding).
    pub fn encode(&self, frame: &Frame, rect: Rect, out: &mut Vec<u8>) {
        let bytes_per_pixel = usize::from(self.bits_per_pixel / 8);
        out.reserve(rect.w * rect.h * bytes_per_pixel);
        for y in rect.y..rect.y + rect.h {
            let start = (y * frame.width + rect.x) * 4;
            let row = &frame.rgba[start..start + rect.w * 4];
            if *self == Self::NATIVE {
                for pixel in row.chunks_exact(4) {
                    out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0]);
                }
                continue;
            }
            for pixel in row.chunks_exact(4) {
                let mut value = 0u32;
                for (channel, &sample) in pixel[..3].iter().enumerate() {
                    let max = u32::from(self.max[channel]);
                    value |= ((u32::from(sample) * max + 127) / 255) << self.shift[channel];
                }
                let bytes = if self.big_endian {
                    value.to_be_bytes()
                } else {
                    value.to_le_bytes()
                };
                let bytes = if self.big_endian {
                    &bytes[4 - bytes_per_pixel..]
                } else {
                    &bytes[..bytes_per_pixel]
                };
                out.extend_from_slice(bytes);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMessage {
    SetPixelFormat(PixelFormat),
    SetEncodings(Vec<i32>),
    /// The requested region is ignored: updates always cover the changed area of the whole window.
    UpdateRequest {
        incremental: bool,
    },
    Key {
        down: bool,
        keysym: u32,
    },
    Pointer {
        buttons: u8,
        x: u16,
        y: u16,
    },
    /// Clipboard text from the client; read and discarded.
    CutText,
    SetDesktopSize(super::resize::Request),
}

fn read_array<const N: usize, R: Read>(reader: &mut R) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub fn read_message<R: Read>(reader: &mut R) -> io::Result<ClientMessage> {
    let [kind] = read_array(reader)?;
    match kind {
        0 => {
            let bytes: [u8; 19] = read_array(reader)?;
            Ok(ClientMessage::SetPixelFormat(PixelFormat::parse(
                bytes[3..].try_into().expect("16 bytes"),
            )?))
        }
        2 => {
            let header: [u8; 3] = read_array(reader)?;
            let count = usize::from(u16::from_be_bytes([header[1], header[2]]));
            if count > MAX_ENCODINGS {
                return Err(invalid("too many encodings"));
            }
            let mut encodings = Vec::with_capacity(count);
            for _ in 0..count {
                encodings.push(i32::from_be_bytes(read_array(reader)?));
            }
            Ok(ClientMessage::SetEncodings(encodings))
        }
        3 => {
            let bytes: [u8; 9] = read_array(reader)?;
            Ok(ClientMessage::UpdateRequest {
                incremental: bytes[0] != 0,
            })
        }
        4 => {
            let bytes: [u8; 7] = read_array(reader)?;
            Ok(ClientMessage::Key {
                down: bytes[0] != 0,
                keysym: u32::from_be_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]),
            })
        }
        5 => {
            let bytes: [u8; 5] = read_array(reader)?;
            Ok(ClientMessage::Pointer {
                buttons: bytes[0],
                x: u16::from_be_bytes([bytes[1], bytes[2]]),
                y: u16::from_be_bytes([bytes[3], bytes[4]]),
            })
        }
        6 => {
            let header: [u8; 7] = read_array(reader)?;
            let length = u32::from_be_bytes([header[3], header[4], header[5], header[6]]) as usize;
            if length > MAX_CUT_TEXT {
                return Err(invalid("clipboard text too large"));
            }
            io::copy(&mut reader.take(length as u64), &mut io::sink())?;
            Ok(ClientMessage::CutText)
        }
        251 => Ok(ClientMessage::SetDesktopSize(super::resize::Request::read(
            reader,
        )?)),
        _ => Err(invalid("unknown client message")),
    }
}

/// Version, security (None) and initialisation exchange. Access control is the SSH
/// tunnel in front of the loopback listener, so no VNC password scheme is offered.
pub fn handshake<S: Read + Write>(
    stream: &mut S,
    width: u16,
    height: u16,
    name: &str,
) -> io::Result<()> {
    stream.write_all(b"RFB 003.008\n")?;
    let version: [u8; 12] = read_array(stream)?;
    if &version[..8] != b"RFB 003." || version[11] != b'\n' {
        return Err(invalid("not an RFB client"));
    }
    let minor: u32 = std::str::from_utf8(&version[8..11])
        .ok()
        .and_then(|digits| digits.parse().ok())
        .ok_or_else(|| invalid("bad RFB version"))?;
    // Clients such as macOS report 3.889; anything below 3.7 is treated as 3.3.
    let minor = if minor >= 8 {
        8
    } else if minor == 7 {
        7
    } else {
        3
    };
    if minor == 3 {
        stream.write_all(&1u32.to_be_bytes())?;
    } else {
        stream.write_all(&[1, SECURITY_NONE])?;
        let [chosen] = read_array(stream)?;
        if chosen != SECURITY_NONE {
            return Err(invalid("client chose an unsupported security type"));
        }
        if minor == 8 {
            stream.write_all(&0u32.to_be_bytes())?;
        }
    }
    let _shared_flag: [u8; 1] = read_array(stream)?;
    let mut init = Vec::new();
    init.extend_from_slice(&width.to_be_bytes());
    init.extend_from_slice(&height.to_be_bytes());
    init.extend_from_slice(&PixelFormat::NATIVE.to_bytes());
    init.extend_from_slice(&(name.len() as u32).to_be_bytes());
    init.extend_from_slice(name.as_bytes());
    stream.write_all(&init)?;
    stream.flush()
}

/// Start a `FramebufferUpdate` carrying `rectangles` rectangles.
pub fn begin_update(out: &mut Vec<u8>, rectangles: u16) {
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&rectangles.to_be_bytes());
}

pub fn rectangle_header(out: &mut Vec<u8>, rect: Rect, encoding: i32) {
    for value in [rect.x, rect.y, rect.w, rect.h] {
        out.extend_from_slice(&(value as u16).to_be_bytes());
    }
    out.extend_from_slice(&encoding.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Reads from a canned client script and records what the server wrote.
    struct Script {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for Script {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn run_handshake(client: &[u8]) -> (io::Result<()>, Vec<u8>) {
        let mut script = Script {
            input: Cursor::new(client.to_vec()),
            output: Vec::new(),
        };
        let result = handshake(&mut script, 1920, 1080, "ANGI");
        (result, script.output)
    }

    #[test]
    fn version_3_8_handshake_ends_with_server_init() {
        let (result, out) = run_handshake(b"RFB 003.008\n\x01\x01");
        result.unwrap();
        assert_eq!(&out[..12], b"RFB 003.008\n");
        assert_eq!(&out[12..14], &[1, 1]); // one security type: None
        assert_eq!(&out[14..18], &[0, 0, 0, 0]); // SecurityResult: OK
        assert_eq!(&out[18..22], &[7, 128, 4, 56]); // 1920x1080
        assert_eq!(&out[22..38], &PixelFormat::NATIVE.to_bytes());
        assert_eq!(&out[38..], b"\0\0\0\x04ANGI");
    }

    #[test]
    fn version_3_3_gets_a_dictated_security_type_and_no_result() {
        let (result, out) = run_handshake(b"RFB 003.003\n\x01");
        result.unwrap();
        assert_eq!(&out[12..16], &[0, 0, 0, 1]);
        assert_eq!(&out[16..20], &[7, 128, 4, 56]);
    }

    #[test]
    fn non_rfb_peers_and_wrong_security_choices_are_rejected() {
        assert!(run_handshake(b"GET / HTTP/1.1\r\n").0.is_err());
        assert!(run_handshake(b"RFB 003.008\n\x02\x01").0.is_err());
    }

    #[test]
    fn pixel_formats_round_trip_and_reject_unsupported_layouts() {
        assert_eq!(
            PixelFormat::parse(&PixelFormat::NATIVE.to_bytes()).unwrap(),
            PixelFormat::NATIVE
        );
        let mut colour_map = PixelFormat::NATIVE.to_bytes();
        colour_map[3] = 0;
        assert!(PixelFormat::parse(&colour_map).is_err());
        let mut odd_depth = PixelFormat::NATIVE.to_bytes();
        odd_depth[0] = 24;
        assert!(PixelFormat::parse(&odd_depth).is_err());
        let mut overflow = PixelFormat::NATIVE.to_bytes();
        overflow[10] = 30; // 255 << 30 does not fit in 32 bits
        assert!(PixelFormat::parse(&overflow).is_err());
    }

    fn one_pixel(r: u8, g: u8, b: u8) -> Frame {
        Frame {
            width: 1,
            height: 1,
            rgba: vec![r, g, b, 255],
        }
    }

    fn encoded(format: PixelFormat, frame: &Frame) -> Vec<u8> {
        let mut out = Vec::new();
        format.encode(frame, frame.bounds(), &mut out);
        out
    }

    #[test]
    fn native_format_is_little_endian_bgrx() {
        assert_eq!(
            encoded(PixelFormat::NATIVE, &one_pixel(1, 2, 3)),
            [3, 2, 1, 0]
        );
    }

    #[test]
    fn reduced_depth_and_big_endian_formats_scale_channels() {
        let rgb565 = PixelFormat {
            bits_per_pixel: 16,
            depth: 16,
            big_endian: false,
            max: [31, 63, 31],
            shift: [11, 5, 0],
        };
        assert_eq!(encoded(rgb565, &one_pixel(255, 255, 255)), [0xff, 0xff]);
        assert_eq!(encoded(rgb565, &one_pixel(255, 0, 0)), [0x00, 0xf8]);
        let big = PixelFormat {
            big_endian: true,
            ..rgb565
        };
        assert_eq!(encoded(big, &one_pixel(255, 0, 0)), [0xf8, 0x00]);
        let bgr233 = PixelFormat {
            bits_per_pixel: 8,
            depth: 8,
            big_endian: false,
            max: [7, 7, 3],
            shift: [0, 3, 6],
        };
        assert_eq!(encoded(bgr233, &one_pixel(255, 255, 255)), [0xff]);
        assert_eq!(encoded(bgr233, &one_pixel(0, 0, 255)), [0xc0]);
    }

    #[test]
    fn only_the_requested_rectangle_is_encoded() {
        let frame = Frame {
            width: 3,
            height: 2,
            rgba: (0..24).collect(),
        };
        let mut out = Vec::new();
        PixelFormat::NATIVE.encode(
            &frame,
            Rect {
                x: 1,
                y: 1,
                w: 2,
                h: 1,
            },
            &mut out,
        );
        assert_eq!(out, [18, 17, 16, 0, 22, 21, 20, 0]);
    }

    #[test]
    fn streaming_pixels_bounds_writes_to_a_row_and_propagates_io_errors() {
        struct Rows {
            bytes: Vec<u8>,
            limit: usize,
        }
        impl Write for Rows {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                assert!(bytes.len() <= self.limit);
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let frame = Frame {
            width: 8,
            height: 4,
            rgba: (0..128).collect(),
        };
        let rect = Rect {
            x: 2,
            y: 1,
            w: 3,
            h: 2,
        };
        let mut rows = Rows {
            bytes: Vec::new(),
            limit: 3 * 4,
        };
        PixelFormat::NATIVE
            .write_pixels(&frame, rect, &mut rows)
            .unwrap();
        let mut expected = Vec::new();
        PixelFormat::NATIVE.encode(&frame, rect, &mut expected);
        assert_eq!(rows.bytes, expected);
        let mut too_small = [0u8; 3];
        assert!(
            PixelFormat::NATIVE
                .write_pixels(&frame, rect, &mut too_small.as_mut_slice())
                .is_err()
        );
    }

    #[test]
    fn client_messages_parse() {
        let mut key = Cursor::new(vec![4, 1, 0, 0, 0, 0, 0xff, 0x0d]);
        assert_eq!(
            read_message(&mut key).unwrap(),
            ClientMessage::Key {
                down: true,
                keysym: 0xff0d
            }
        );
        let mut pointer = Cursor::new(vec![5, 0b101, 0, 10, 0, 20]);
        assert_eq!(
            read_message(&mut pointer).unwrap(),
            ClientMessage::Pointer {
                buttons: 5,
                x: 10,
                y: 20
            }
        );
        let mut request = Cursor::new(vec![3, 1, 0, 0, 0, 0, 0, 100, 0, 100]);
        assert_eq!(
            read_message(&mut request).unwrap(),
            ClientMessage::UpdateRequest { incremental: true }
        );
        let mut encodings = Cursor::new(vec![2, 0, 0, 2, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0x21]);
        assert_eq!(
            read_message(&mut encodings).unwrap(),
            ClientMessage::SetEncodings(vec![0, ENCODING_DESKTOP_SIZE])
        );
    }

    #[test]
    fn clipboard_text_is_consumed_so_the_stream_stays_in_sync() {
        let mut stream = Cursor::new(vec![
            6, 0, 0, 0, 0, 0, 0, 3, b'a', b'b', b'c', 3, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        assert_eq!(read_message(&mut stream).unwrap(), ClientMessage::CutText);
        assert!(matches!(
            read_message(&mut stream).unwrap(),
            ClientMessage::UpdateRequest { incremental: false }
        ));
    }

    #[test]
    fn hostile_messages_are_errors_not_allocations() {
        assert!(read_message(&mut Cursor::new(vec![9])).is_err());
        assert!(read_message(&mut Cursor::new(vec![2, 0, 0xff, 0xff])).is_err());
        assert!(read_message(&mut Cursor::new(vec![6, 0, 0, 0, 0xff, 0xff, 0xff, 0xff])).is_err());
        assert!(read_message(&mut Cursor::new(vec![4, 1])).is_err());
    }
}
