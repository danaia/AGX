fn main() {
    if let Err(error) = agx_net::service::dhcp_hook() {
        eprintln!("agx-dhcp: {error}");
        std::process::exit(1);
    }
}
