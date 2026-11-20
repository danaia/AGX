//! Small, bounded HTTP transport for the point-to-point USB setup page.
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const LIMIT: usize = 8192;
const TIMEOUT: Duration = Duration::from_secs(4);

pub enum Request {
    Page,
    Script,
    Styles,
    Brand,
    Scan(Zeroizing<Vec<u8>>),
    Claim(Zeroizing<Vec<u8>>),
    NotFound,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid HTTP setup request")
}

fn parse_head(head: &str) -> io::Result<(&str, &str, usize)> {
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().ok_or_else(invalid)?.split(' ');
    let method = request_line.next().ok_or_else(invalid)?;
    let path = request_line.next().ok_or_else(invalid)?;
    if request_line.next() != Some("HTTP/1.1") || request_line.next().is_some() {
        return Err(invalid());
    }
    let mut host = None;
    let mut origin = None;
    let mut content_type = None;
    let mut length = None;
    for header in lines.filter(|line| !line.is_empty()) {
        let (name, value) = header.split_once(':').ok_or_else(invalid)?;
        if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(invalid());
        }
        let value = value.trim();
        let field = match name.to_ascii_lowercase().as_str() {
            "host" => &mut host,
            "origin" => &mut origin,
            "content-type" => &mut content_type,
            "content-length" => &mut length,
            "transfer-encoding" | "expect" => return Err(invalid()),
            _ => continue,
        };
        if field.replace(value).is_some() {
            return Err(invalid());
        }
    }
    let host = host
        .filter(|host| matches!(*host, "10.77.0.2" | "angi.local"))
        .ok_or_else(invalid)?;
    let length = length
        .map(|value| value.parse::<usize>().map_err(|_| invalid()))
        .transpose()?
        .unwrap_or_default();
    if method == "POST" {
        if !(1..=LIMIT).contains(&length)
            || content_type != Some("application/json")
            || origin != Some(format!("http://{host}").as_str())
        {
            return Err(invalid());
        }
    } else if length != 0 {
        return Err(invalid());
    }
    Ok((method, path, length))
}

pub fn read(stream: &mut TcpStream) -> io::Result<Request> {
    let deadline = Instant::now() + TIMEOUT;
    let mut bytes = Zeroizing::new(Vec::with_capacity(LIMIT * 2 + 2048));
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || bytes.len() >= LIMIT * 2 {
            return Err(invalid());
        }
        stream.set_read_timeout(Some(remaining))?;
        let mut chunk = Zeroizing::new([0; 2048]);
        let received = stream.read(chunk.as_mut())?;
        if received == 0 {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk[..received]);
        let Some(head_end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") else {
            if bytes.len() > LIMIT {
                return Err(invalid());
            }
            continue;
        };
        if head_end + 4 > LIMIT {
            return Err(invalid());
        }
        let head = std::str::from_utf8(&bytes[..head_end]).map_err(|_| invalid())?;
        let (method, path, length) = parse_head(head)?;
        let body_at = head_end + 4;
        if bytes.len() < body_at + length {
            continue;
        }
        if bytes.len() != body_at + length {
            return Err(invalid());
        }
        return Ok(match (method, path) {
            ("GET", "/") => Request::Page,
            ("GET", "/setup.js") => Request::Script,
            ("GET", "/setup.css") => Request::Styles,
            ("GET", "/brand-bug.svg") => Request::Brand,
            ("POST", "/scan") => Request::Scan(Zeroizing::new(bytes[body_at..].to_vec())),
            ("POST", "/claim") => Request::Claim(Zeroizing::new(bytes[body_at..].to_vec())),
            _ => Request::NotFound,
        });
    }
}

pub fn send(stream: &mut TcpStream, status: &str, kind: &str, bytes: &[u8]) -> io::Result<()> {
    stream.set_write_timeout(Some(TIMEOUT))?;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Connection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; \
         connect-src 'self'; form-action 'none'; frame-ancestors 'none'; base-uri 'none'\r\n\r\n",
        bytes.len(),
    )?;
    stream.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_require_the_same_usb_origin_and_unambiguous_framing() {
        let post = "POST /claim HTTP/1.1\r\nHost: angi.local\r\nOrigin: http://angi.local\r\nContent-Type: application/json\r\nContent-Length: 55";
        assert_eq!(parse_head(post).unwrap(), ("POST", "/claim", 55));
        for request in [
            post.replace(
                "Origin: http://angi.local",
                "Origin: https://elsewhere.test",
            ),
            post.replace("Host: angi.local", "Host: elsewhere.test"),
            post.replace("Content-Length: 55", "Content-Length: 999999"),
            format!("{post}\r\nContent-Length: 55"),
            format!("{post}\r\nTransfer-Encoding: chunked"),
            post.replace("Content-Type: application/json", "Content-Type: text/plain"),
        ] {
            assert!(parse_head(&request).is_err(), "{request}");
        }
    }
}
