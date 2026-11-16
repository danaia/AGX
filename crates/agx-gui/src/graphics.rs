//! Explicit renderer selection keeps GPU/software comparisons honest and cursors singular.
#[cfg(any(feature = "kms", test))]
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

pub const ENVIRONMENT: [&str; 5] = [
    "SLINT_BACKEND",
    "SLINT_DRM_MODE",
    "SLINT_DRM_OUTPUT",
    "SLINT_DEBUG_PERFORMANCE",
    crate::remote::ENV,
];
static SOFTWARE_CURSOR: AtomicBool = AtomicBool::new(false);

/// Called before signal handlers, Slint, or any application worker starts threads.
pub fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "kms")]
    if unsafe { libc::geteuid() } == 0 {
        // /etc/agx is private to root; the greeter passes these whitelisted
        // options to the authenticated session instead of opening it as that user.
        match std::fs::read_to_string("/etc/agx/gui.env") {
            Ok(config) => {
                for (key, value) in parse_config(&config)? {
                    if std::env::var_os(&key).is_none() {
                        // Only whitelisted Slint options; this file is data, never shell code.
                        unsafe {
                            std::env::set_var(key, value);
                        }
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let backend = std::env::var("SLINT_BACKEND").unwrap_or_else(|_| {
        if cfg!(feature = "kms") {
            "linuxkms-femtovg"
        } else {
            "winit-femtovg"
        }
        .into()
    });
    // Explicit selection prevents silent software fallback from masquerading as a GPU result.
    if !matches!(
        backend.as_str(),
        "linuxkms-femtovg" | "linuxkms-software" | "winit-femtovg" | "winit-software"
    ) {
        return Err(format!("Unsupported SLINT_BACKEND: {backend}").into());
    }
    slint::BackendSelector::new()
        .backend_name(backend.clone())
        .select()?;
    SOFTWARE_CURSOR.store(backend == "linuxkms-software", Ordering::Relaxed);
    eprintln!("agx-gui: backend={backend}");
    Ok(())
}

pub fn draw_cursor() -> bool {
    SOFTWARE_CURSOR.load(Ordering::Relaxed) || std::env::var_os("AGX_GUI_CURSOR").is_some()
}

#[cfg(any(feature = "kms", test))]
fn parse_config(config: &str) -> io::Result<Vec<(String, String)>> {
    let mut options = Vec::new();
    for line in config
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| io::Error::other("Expected KEY=value in /etc/agx/gui.env"))?;
        let (key, value) = (key.trim(), value.trim());
        if !ENVIRONMENT.contains(&key) || value.is_empty() || value.contains('\0') {
            return Err(io::Error::other(
                "Invalid graphics option in /etc/agx/gui.env",
            ));
        }
        options.push((key.into(), value.into()));
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_only_accepts_graphics_options_and_never_executes_shell() {
        let options = parse_config(
            "# Comparison run\nSLINT_BACKEND=linuxkms-software\nSLINT_DRM_MODE=4\nAGX_GUI_VNC=on\n",
        )
        .unwrap();
        assert_eq!(
            options[0],
            ("SLINT_BACKEND".into(), "linuxkms-software".into())
        );
        assert!(parse_config("LD_PRELOAD=/tmp/untrusted.so").is_err());
        assert!(parse_config("export SLINT_BACKEND=linuxkms-software").is_err());
        assert!(parse_config("SLINT_BACKEND=").is_err());
    }
}
