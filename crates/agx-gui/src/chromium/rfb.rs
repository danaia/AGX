//! Raw framebuffer and input messages over the private WayVNC socket.
use crate::remote::rfb::PixelFormat;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

pub(super) type Frame = SharedPixelBuffer<Rgba8Pixel>;

pub(super) enum Input {
    Key(u32, bool),
    Pointer(u16, u16, u8),
}

impl Input {
    pub(super) fn write(self, socket: &mut UnixStream) -> io::Result<()> {
        match self {
            Self::Key(symbol, down) => {
                let mut packet = [4, u8::from(down), 0, 0, 0, 0, 0, 0];
                packet[4..].copy_from_slice(&symbol.to_be_bytes());
                socket.write_all(&packet)
            }
            Self::Pointer(x, y, buttons) => {
                let [xh, xl] = x.to_be_bytes();
                let [yh, yl] = y.to_be_bytes();
                socket.write_all(&[5, buttons, xh, xl, yh, yl])
            }
        }
    }
}

pub(super) struct Client {
    pub(super) socket: UnixStream,
    width: u16,
    height: u16,
    frame: Frame,
}

fn read<const N: usize>(socket: &mut UnixStream) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    socket.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

impl Client {
    pub(super) fn connect(mut socket: UnixStream) -> io::Result<Self> {
        if read::<12>(&mut socket)? != *b"RFB 003.008\n" {
            return Err(invalid("unsupported framebuffer protocol"));
        }
        socket.write_all(b"RFB 003.008\n")?;
        let [size] = read::<1>(&mut socket)?;
        let mut options = vec![0; size.into()];
        socket.read_exact(&mut options)?;
        if !options.contains(&1) {
            return Err(invalid("private framebuffer connection refused"));
        }
        socket.write_all(&[1])?;
        if read::<4>(&mut socket)? != [0; 4] {
            return Err(invalid("private framebuffer handshake failed"));
        }
        socket.write_all(&[1])?;
        let [a, b, c, d] = read::<4>(&mut socket)?;
        let (width, height) = (u16::from_be_bytes([a, b]), u16::from_be_bytes([c, d]));
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(invalid("invalid framebuffer size"));
        }
        read::<16>(&mut socket)?;
        let name_len = u32::from_be_bytes(read::<4>(&mut socket)?);
        discard(&mut socket, name_len)?;
        socket.write_all(&[0; 4])?;
        socket.write_all(
            &PixelFormat {
                shift: [0, 8, 16],
                ..PixelFormat::NATIVE
            }
            .to_bytes(),
        )?;
        // Advertise Raw alone. The compositor keeps a fixed output size and ANGI scales the image.
        socket.write_all(&[2, 0, 0, 1, 0, 0, 0, 0])?;
        let frame = Frame::new(width.into(), height.into());
        Ok(Self {
            socket,
            width,
            height,
            frame,
        })
    }

    pub(super) fn request(&mut self, incremental: bool) -> io::Result<()> {
        let [wh, wl] = self.width.to_be_bytes();
        let [hh, hl] = self.height.to_be_bytes();
        self.socket
            .write_all(&[3, u8::from(incremental), 0, 0, 0, 0, wh, wl, hh, hl])
    }

    pub(super) fn receive(&mut self) -> io::Result<Option<Frame>> {
        match read::<1>(&mut self.socket)?[0] {
            0 => self.receive_frame().map(Some),
            2 => Ok(None), // Bell
            3 => {
                read::<3>(&mut self.socket)?;
                let size = u32::from_be_bytes(read::<4>(&mut self.socket)?);
                discard(&mut self.socket, size)?;
                Ok(None)
            }
            _ => Err(invalid("unexpected framebuffer message")),
        }
    }

    fn receive_frame(&mut self) -> io::Result<Frame> {
        let [_, hi, lo] = read::<3>(&mut self.socket)?;
        for _ in 0..u16::from_be_bytes([hi, lo]) {
            let [a, b, c, d, e, f, g, h, i, j, k, l] = read::<12>(&mut self.socket)?;
            let [x, y, w, h] =
                [[a, b], [c, d], [e, f], [g, h]].map(|pair| usize::from(u16::from_be_bytes(pair)));
            let encoding = i32::from_be_bytes([i, j, k, l]);
            let stride = usize::from(self.width);
            if encoding != 0 || x + w > stride || y + h > usize::from(self.height) {
                return Err(invalid("invalid raw framebuffer rectangle"));
            }
            let pixels = self.frame.make_mut_bytes();
            for row in y..y + h {
                let start = 4 * (row * stride + x);
                let bytes = &mut pixels[start..start + 4 * w];
                self.socket.read_exact(bytes)?;
                for pixel in bytes.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
            }
        }
        Ok(self.frame.clone())
    }
}

fn discard(socket: &mut UnixStream, length: u32) -> io::Result<()> {
    if length > 1024 * 1024 {
        return Err(invalid("oversize framebuffer message"));
    }
    if io::copy(&mut socket.take(length.into()), &mut io::sink())? != u64::from(length) {
        return Err(invalid("truncated framebuffer message"));
    }
    Ok(())
}
