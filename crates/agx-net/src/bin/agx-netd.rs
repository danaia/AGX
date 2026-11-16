fn main() {
    if let Err(error) = agx_net::service::run() {
        eprintln!("agx-netd: {error}");
        std::process::exit(1);
    }
}
