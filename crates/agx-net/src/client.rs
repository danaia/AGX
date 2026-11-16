//! Bounded Unix-socket request/response client.
use crate::{Reply, Request, SOCKET};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};
use zeroize::Zeroizing;

pub fn request(command: &Request) -> io::Result<Reply> {
    let mut connection = UnixStream::connect(SOCKET).map_err(|e| {
        io::Error::new(
            e.kind(),
            "Wi-Fi service unavailable. Check that agx-netd is installed and running as root.",
        )
    })?;
    connection.set_read_timeout(Some(Duration::from_secs(8)))?;
    connection.set_write_timeout(Some(Duration::from_secs(3)))?;
    let data = Zeroizing::new(serde_json::to_vec(command)?);
    connection.write_all(&data)?;
    connection.write_all(b"\n")?;
    let mut response = Vec::new();
    BufReader::new(connection)
        .take(1024 * 1024)
        .read_until(b'\n', &mut response)?;
    serde_json::from_slice(&response)
        .map_err(|_| io::Error::other("Invalid Wi-Fi service response"))
}
