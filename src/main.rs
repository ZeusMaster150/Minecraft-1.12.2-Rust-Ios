fn main() -> anyhow::Result<()> {
    // Show the renderer/network lifecycle without requiring users to know the
    // RUST_LOG environment variable. An explicit RUST_LOG value still wins.
    let environment = env_logger::Env::default().filter_or("RUST_LOG", "info");

    #[cfg(target_os = "ios")]
    install_ios_logging(environment);

    #[cfg(not(target_os = "ios"))]
    env_logger::Builder::from_env(environment)
        .format_timestamp_millis()
        .init();

    install_panic_logging();

    log::info!(
        "Minecraft 1.12.2 Rust dual-renderer client package {}",
        env!("CARGO_PKG_VERSION")
    );
    minecraft_1_12_2_rust_vulkan::net::minecraft::client::main::Main::main(
        std::env::args_os(),
    )
}

/// Records panics through the logger. With `panic = "abort"` the process dies
/// immediately, so this hook is the only place a message reaches the log file.
fn install_panic_logging() {
    std::panic::set_hook(Box::new(|info| {
        log::error!("fatal panic: {info}");
    }));
}

/// iOS apps launched from SpringBoard have no visible stdout or stderr, so the
/// log is written to `Documents/latest.log` next to the game directory. That
/// file is reachable over Finder/iTunes file sharing.
#[cfg(target_os = "ios")]
fn install_ios_logging(environment: env_logger::Env) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_owned());
    let documents = std::path::PathBuf::from(home).join("Documents");
    let _ = std::fs::create_dir_all(&documents);
    let log_path = documents.join("latest.log");

    let mut builder = env_logger::Builder::from_env(environment);
    builder.format_timestamp_millis();
    match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
    {
        Ok(file) => {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
        Err(error) => {
            // Nothing else can be done without a writable sandbox; the system
            // log still receives stderr when the app is started by a debugger.
            eprintln!("failed to open {}: {error}", log_path.display());
        }
    }
    builder.init();
}
