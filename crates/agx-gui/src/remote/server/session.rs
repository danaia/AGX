//! Established VNC sessions: framebuffer streaming, resize replies and client input.
use super::{Job, Shared};
use crate::remote::{
    frame::{Frame, changed_bounds},
    input::{Input, pointer_inputs},
    resize::{self, Reply},
    rfb::{self, ClientMessage, ENCODING_DESKTOP_SIZE, ENCODING_RAW},
};
use std::{
    io::{self, BufWriter, Write},
    net::TcpStream,
    sync::Arc,
    thread,
};

/// Serve an established connection: read its input on one thread and send it updates
/// from this one, until either side ends. `known_size` is the picture the viewer has.
pub(super) fn stream_updates(
    stream: &TcpStream,
    shared: &Arc<Shared>,
    id: u64,
    known_size: (usize, usize),
) -> io::Result<()> {
    let size = |frame: &Frame| -> io::Result<(u16, u16)> {
        Ok((
            u16::try_from(frame.width).map_err(io::Error::other)?,
            u16::try_from(frame.height).map_err(io::Error::other)?,
        ))
    };
    let kept = stream.try_clone()?;
    shared.update_client(id, |client| {
        client.stream = Some(kept);
        client.size = known_size;
    });
    let reader = stream.try_clone()?;
    let reader_shared = shared.clone();
    thread::Builder::new()
        .name("vnc-input".into())
        .spawn(move || {
            let _ = read_loop(reader, &reader_shared, id);
            reader_shared.update_client(id, |client| client.closed = true);
        })?;

    // Bound encoded output to 64 KiB instead of another full RGBA frame per viewer.
    let mut writer = BufWriter::with_capacity(64 * 1024, stream);
    let mut known_size = known_size;
    let mut sent: Option<Arc<Frame>> = None;
    let mut seen = 0;
    while let Some(job) = shared.next_job(id, seen) {
        let Job::Pixels {
            frame,
            sequence,
            full,
            format,
            desktop_size,
            extended_desktop_size,
        } = job
        else {
            let Job::Layout(layout) = job else {
                unreachable!()
            };
            writer.write_all(&layout.encode())?;
            writer.flush()?;
            if layout.status == 0 {
                known_size = (usize::from(layout.width), usize::from(layout.height));
                sent = None;
            }
            continue;
        };
        seen = sequence;
        let resized = (frame.width, frame.height) != known_size;
        let area = match (&sent, full || resized) {
            (Some(previous), false) => changed_bounds(previous, &frame),
            _ => Some(frame.bounds()),
        };
        let Some(area) = area else {
            sent = Some(frame); // Nothing visible changed: keep the request pending.
            shared.update_client(id, |client| client.pending = true);
            continue;
        };
        let mut message = Vec::new();
        if extended_desktop_size && (full || resized) {
            let (width, height) = size(&frame)?;
            writer.write_all(
                &Reply {
                    reason: 0,
                    status: 0,
                    width,
                    height,
                }
                .encode(),
            )?;
        }
        if resized && !extended_desktop_size {
            if !desktop_size {
                return Err(io::Error::other(
                    "window size changed and the client cannot resize",
                ));
            }
            size(&frame)?;
            rfb::begin_update(&mut message, 2);
            rfb::rectangle_header(&mut message, frame.bounds(), ENCODING_DESKTOP_SIZE);
        } else {
            rfb::begin_update(&mut message, 1);
        }
        rfb::rectangle_header(&mut message, area, ENCODING_RAW);
        writer.write_all(&message)?;
        format.write_pixels(&frame, area, &mut writer)?;
        writer.flush()?;
        known_size = (frame.width, frame.height);
        sent = Some(frame);
        shared.update_client(id, |client| {
            client.size = known_size;
        });
    }
    Ok(())
}

fn read_loop(stream: TcpStream, shared: &Arc<Shared>, id: u64) -> io::Result<()> {
    // Unbuffered on purpose: if the process is replaced mid-way, no half-read
    // message is lost to a read-ahead buffer.
    let mut reader = &stream;
    let mut buttons = 0;
    loop {
        match rfb::read_message(&mut reader)? {
            ClientMessage::SetPixelFormat(format) => {
                shared.update_client(id, |client| client.format = format)
            }
            ClientMessage::SetEncodings(encodings) => {
                shared.update_client(id, |client| {
                    client.desktop_size = encodings.contains(&ENCODING_DESKTOP_SIZE);
                    client.extended_desktop_size = encodings.contains(&resize::ENCODING);
                });
            }
            ClientMessage::UpdateRequest { incremental } => shared.update_client(id, |client| {
                client.pending = true;
                client.full |= !incremental;
            }),
            ClientMessage::Key { down, keysym } => {
                shared.note_input();
                (shared.input)(Input::Key { down, keysym });
            }
            ClientMessage::Pointer { buttons: now, x, y } => {
                shared.note_input();
                for input in pointer_inputs(buttons, now, x, y) {
                    (shared.input)(input);
                }
                buttons = now;
            }
            ClientMessage::CutText => {}
            ClientMessage::SetDesktopSize(request) => shared.resize_desktop(id, request)?,
        }
    }
}
