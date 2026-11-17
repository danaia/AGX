//! One bounded DMA-BUF descriptor per sequenced packet, separate from the control stream.
use super::super::protocol::{HEIGHT, WIDTH};
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};
#[cfg_attr(not(feature = "kms"), allow(dead_code))]
#[derive(Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub stride: u32,
    pub offset: u32,
    pub modifier: u64,
    pub sequence: u64,
    pub fd: OwnedFd,
}
pub fn pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1; 2];
    if unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}
fn decode(bytes: &[u8], fd: OwnedFd) -> io::Result<Frame> {
    if bytes.len() != 44 || &bytes[..8] != b"ANGIGPU1" {
        return Err(io::Error::other("invalid GPU frame header"));
    }
    let word = |offset| u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let width = word(8);
    let height = word(12);
    let format = word(16);
    let stride = word(20);
    let offset = word(24);
    let modifier = u64::from_be_bytes(bytes[28..36].try_into().unwrap());
    let sequence = u64::from_be_bytes(bytes[36..44].try_into().unwrap());
    if width == 0
        || height == 0
        || width > WIDTH
        || height > HEIGHT
        || sequence == 0
        || !matches!(format, 0x34325241 | 0x34325258)
        || stride < width * 4
        || stride > WIDTH * 16
        || (offset as u64 + stride as u64 * height as u64) > 256 * 1024 * 1024
    {
        return Err(io::Error::other("invalid GPU frame geometry or format"));
    }
    Ok(Frame {
        width,
        height,
        format,
        stride,
        offset,
        modifier,
        sequence,
        fd,
    })
}
pub fn receive(socket: &OwnedFd) -> io::Result<Frame> {
    let mut bytes = [0u8; 44];
    // Aligned storage with room to close excess descriptors even in an invalid packet.
    let mut control = [0usize; 16];
    let mut iov = libc::iovec {
        iov_base: bytes.as_mut_ptr().cast(),
        iov_len: bytes.len(),
    };
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = std::mem::size_of_val(&control);
    let size = loop {
        let size = unsafe {
            libc::recvmsg(
                socket.as_raw_fd(),
                &mut message,
                libc::MSG_CMSG_CLOEXEC | libc::MSG_TRUNC,
            )
        };
        if size < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        break size;
    };
    if size < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut fds = Vec::new();
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&message);
        while !header.is_null() {
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                let length = (*header)
                    .cmsg_len
                    .saturating_sub(libc::CMSG_LEN(0) as usize);
                for index in 0..length / std::mem::size_of::<i32>() {
                    let fd =
                        std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<i32>().add(index));
                    fds.push(OwnedFd::from_raw_fd(fd));
                }
            }
            header = libc::CMSG_NXTHDR(&message, header);
        }
    }
    if size == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    if size != bytes.len() as isize
        || message.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0
        || fds.len() != 1
    {
        return Err(io::Error::other("invalid GPU frame descriptors"));
    }
    decode(&bytes, fds.remove(0))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, os::unix::net::UnixStream};
    fn descriptor() -> OwnedFd {
        std::fs::File::open("/dev/null").unwrap().into()
    }
    #[test]
    fn frame_geometry_and_format_are_bounded() {
        let mut bytes = [0u8; 44];
        bytes[..8].copy_from_slice(b"ANGIGPU1");
        for (offset, value) in [(8, 1280u32), (12, 720), (16, 0x34325241), (20, 5120)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[36..44].copy_from_slice(&1u64.to_be_bytes());
        assert_eq!(decode(&bytes, descriptor()).unwrap().width, 1280);
        bytes[20..24].copy_from_slice(&4u32.to_be_bytes());
        let (mut reader, writer) = UnixStream::pair().unwrap();
        assert!(decode(&bytes, writer.into()).is_err());
        // Observe EOF on the resource, not a raw fd number which another
        // concurrently running test may immediately reuse after close.
        assert_eq!(reader.read(&mut [0u8; 1]).unwrap(), 0);
    }
    #[test]
    fn sequenced_packet_transfers_one_close_on_exec_descriptor() {
        let (reader, writer) = pair().unwrap();
        let original = descriptor();
        let mut bytes = [0u8; 44];
        bytes[..8].copy_from_slice(b"ANGIGPU1");
        for (offset, value) in [(8, 64u32), (12, 64), (16, 0x34325241), (20, 256)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[36..44].copy_from_slice(&99u64.to_be_bytes());
        let mut control = [0usize; 4];
        let mut iov = libc::iovec {
            iov_base: bytes.as_mut_ptr().cast(),
            iov_len: bytes.len(),
        };
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = unsafe { libc::CMSG_SPACE(4) as usize };
        unsafe {
            let header = libc::CMSG_FIRSTHDR(&message);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(4) as usize;
            std::ptr::write_unaligned(libc::CMSG_DATA(header).cast::<i32>(), original.as_raw_fd());
            assert_eq!(libc::sendmsg(writer.as_raw_fd(), &message, 0), 44);
        }
        let frame = receive(&reader).unwrap();
        assert_eq!(frame.sequence, 99);
        assert_ne!(frame.fd.as_raw_fd(), original.as_raw_fd());
        assert_ne!(
            unsafe { libc::fcntl(frame.fd.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        assert!(unsafe { libc::fcntl(original.as_raw_fd(), libc::F_GETFD) } >= 0);
    }
    #[test]
    fn invalid_packets_without_fds_are_rejected() {
        let (reader, writer) = pair().unwrap();
        let bytes = [0u8; 44];
        assert_eq!(
            unsafe { libc::send(writer.as_raw_fd(), bytes.as_ptr().cast(), 44, 0) },
            44
        );
        assert!(receive(&reader).is_err());
    }
}
