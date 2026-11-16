//! Pure parsers and validation for Linux CPU and memory counters.
use crate::{CpuTicks, Memory};
use std::io;

pub fn parse_cpu(input: &str) -> io::Result<CpuTicks> {
    let line = input
        .lines()
        .find(|s| s.starts_with("cpu "))
        .ok_or_else(|| invalid("Missing CPU counters"))?;
    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(|s| s.parse().map_err(|_| invalid("Invalid CPU counter")))
        .collect::<io::Result<_>>()?;
    if values.len() < 4 {
        return Err(invalid("Incomplete CPU counters"));
    }
    let total = values
        .iter()
        .try_fold(0u64, |sum, n| sum.checked_add(*n))
        .ok_or_else(|| invalid("CPU counter overflow"))?;
    let idle = values[3]
        .checked_add(*values.get(4).unwrap_or(&0))
        .ok_or_else(|| invalid("CPU idle counter overflow"))?;
    Ok(CpuTicks { total, idle })
}

pub fn cpu_usage(previous: CpuTicks, current: CpuTicks) -> Option<f64> {
    let total = current.total.checked_sub(previous.total)?;
    let idle = current.idle.checked_sub(previous.idle)?;
    if total == 0 || idle > total {
        return None;
    }
    Some((total - idle) as f64 / total as f64 * 100.0)
}

pub fn parse_memory(input: &str) -> io::Result<Memory> {
    let get = |key: &str| -> io::Result<u64> {
        let line = input
            .lines()
            .find(|line| line.starts_with(key))
            .ok_or_else(|| invalid("Missing memory counter"))?;
        let mut fields = line.split_whitespace().skip(1);
        let amount: u64 = fields
            .next()
            .ok_or_else(|| invalid("Missing memory value"))?
            .parse()
            .map_err(|_| invalid("Invalid memory value"))?;
        if fields.next() != Some("kB") {
            return Err(invalid("Unsupported memory unit"));
        }
        amount
            .checked_mul(1024)
            .ok_or_else(|| invalid("Memory counter overflow"))
    };
    let memory = Memory {
        total_bytes: get("MemTotal:")?,
        available_bytes: get("MemAvailable:")?,
    };
    if memory.total_bytes == 0 || memory.available_bytes > memory.total_bytes {
        return Err(invalid("Inconsistent memory counters"));
    }
    Ok(memory)
}

pub(super) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
