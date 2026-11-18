//! Bounded Unix-socket request/response client.
use crate::{Reply, Request, SOCKET, SOCKET_ENV};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

pub fn socket_path() -> PathBuf {
    std::env::var_os(SOCKET_ENV)
        .filter(|path| !path.is_empty())
        .map_or_else(|| PathBuf::from(SOCKET), PathBuf::from)
}

pub fn request(command: &Request) -> io::Result<Reply> {
    let mut connection = UnixStream::connect(socket_path()).map_err(|error| {
        io::Error::new(
            error.kind(),
            "Bluetooth service unavailable. Check that agx-bluetoothd is installed and running.",
        )
    })?;
    // Pairing waits on the radio, so allow longer than the Wi-Fi client does.
    connection.set_read_timeout(Some(Duration::from_secs(40)))?;
    connection.set_write_timeout(Some(Duration::from_secs(3)))?;
    serde_json::to_writer(&mut connection, command)?;
    connection.write_all(b"\n")?;
    let mut response = Vec::new();
    BufReader::new(connection)
        .take(1024 * 1024)
        .read_until(b'\n', &mut response)?;
    serde_json::from_slice(&response)
        .map_err(|_| io::Error::other("Invalid Bluetooth service response"))
}
