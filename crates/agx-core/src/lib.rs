//! Read-only Linux system information shared by AGX interfaces.
mod models;
mod parsers;
mod sampler;

pub use models::{CpuTicks, Hardware, Memory, Network, Snapshot, Storage};
pub use parsers::{cpu_usage, parse_cpu, parse_memory};
pub use sampler::Sampler;

#[cfg(test)]
mod tests;
