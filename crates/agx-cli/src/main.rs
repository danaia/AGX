use agx_core::Sampler;
use agx_tui::{ColorMode, Options};
use std::{env, process::ExitCode, time::Duration};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agx: {error}");
            ExitCode::FAILURE
        }
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("tui") => {
            let mut options = Options::default();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--ascii" => options.ascii = true,
                    "--no-mouse" => options.mouse = false,
                    "--no-color" => options.color = ColorMode::Never,
                    "--color" => {
                        options.color = match args.next().as_deref() {
                            Some("auto") => ColorMode::Auto,
                            Some("truecolor") => ColorMode::TrueColor,
                            Some("256") => ColorMode::Indexed,
                            Some("16") => ColorMode::Basic,
                            Some("never") => ColorMode::Never,
                            _ => {
                                return Err(
                                    "--color expects auto, truecolor, 256, 16, or never".into()
                                );
                            }
                        }
                    }
                    "--help" | "-h" => {
                        help();
                        return Ok(());
                    }
                    _ => return Err(format!("unknown TUI option: {arg}").into()),
                }
            }
            agx_tui::run(options)?;
        }
        Some("wifi") => {
            let command = match args.next().as_deref() {
                None | Some("status") => agx_net::Request::Status,
                Some("scan") => agx_net::Request::Scan,
                Some("check") => agx_net::Request::Check,
                _ => return Err("usage: agx wifi [status|scan|check] [--json]".into()),
            };
            let flags: Vec<_> = args.collect();
            if flags.iter().any(|flag| flag != "--json") {
                return Err("usage: agx wifi [status|scan|check] [--json]".into());
            }
            let reply = agx_net::request(&command)?;
            if flags.iter().any(|flag| flag == "--json") {
                println!("{}", serde_json::to_string(&reply.status)?);
            } else {
                println!("{}: {}", reply.status.phase, reply.message);
            }
            if !reply.ok {
                return Err(reply.message.into());
            }
        }
        Some("status") => {
            let flags: Vec<_> = args.collect();
            if flags.iter().any(|flag| flag != "--json") {
                return Err("usage: agx status [--json]".into());
            }
            let mut sampler = Sampler::default();
            sampler.collect()?;
            std::thread::sleep(Duration::from_millis(200));
            let snapshot = sampler.collect()?;
            if flags.iter().any(|flag| flag == "--json") {
                println!("{}", serde_json::to_string(&snapshot)?);
            } else {
                println!("{} / {}", snapshot.hostname, snapshot.board);
                println!(
                    "Kernel: {} | Uptime: {}s | CPUs: {}",
                    snapshot.kernel, snapshot.uptime_secs, snapshot.cpu_count
                );
                println!(
                    "Memory available: {} / {} MiB",
                    snapshot.memory.available_bytes / 1048576,
                    snapshot.memory.total_bytes / 1048576
                );
                for network in snapshot.networks {
                    println!(
                        "{}: {} [{}]",
                        network.name,
                        network.ipv4.join(", "),
                        network.state
                    );
                }
                for warning in snapshot.warnings {
                    eprintln!("warning: {warning}");
                }
            }
        }
        Some("--version") | Some("-V") => println!("agx {}", env!("CARGO_PKG_VERSION")),
        None | Some("help") | Some("--help") | Some("-h") => help(),
        Some(command) => return Err(format!("unknown command: {command}; use agx --help").into()),
    }
    Ok(())
}
fn help() {
    println!(
        "AGX system console\n\n  agx tui [--ascii] [--no-color] [--no-mouse]\n          [--color auto|truecolor|256|16|never]\n  agx status [--json]\n  agx wifi [status|scan|check] [--json]\n  agx --version\n\nTUI: click tabs and hardware rows; scroll with the mouse wheel.\nKeyboard: 1 Profile, 2 Overview, 3 Hardware, 4 Network, 5 System, ? Help, Tab switch, arrows scroll, r refresh, F10 reboot, q quit.\nRun over an interactive SSH terminal. NO_COLOR is respected in auto mode."
    );
}
