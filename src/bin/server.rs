//! RustyPotato server binary
//!
//! Main entry point for the RustyPotato server with full command engine and network integration.
//! Provides graceful startup with configuration loading, comprehensive signal handling for
//! graceful shutdown, and integration of all components (storage, network, persistence, config).

use clap::{Arg, Command};
use rustypotato::config::LogFormat;
use rustypotato::monitoring::{LogRotationConfig, RotationPolicy};
use rustypotato::{Config, HealthChecker, LogRotationManager, MonitoringServer, RustyPotatoServer};
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

/// Server shutdown coordinator for managing graceful shutdown.
///
/// Signal handling is fanned out via `tokio::select!` against a future
/// returned from `setup_signal_handlers`. There used to be a
/// `broadcast::Sender<()>` here that the signal handler `send`-d to
/// at the end, but it had no subscribers — the actual fan-out was
/// always the future completing. The broadcast was theatrical (its
/// own log line read "Failed to send shutdown signal (no receivers)")
/// and has been removed.
struct ShutdownCoordinator {
    server: Option<RustyPotatoServer>,
    monitoring_server: Option<MonitoringServer>,
    log_rotation: Option<Arc<LogRotationManager>>,
}

impl ShutdownCoordinator {
    fn new(
        server: RustyPotatoServer,
        monitoring_server: MonitoringServer,
        log_rotation: Arc<LogRotationManager>,
    ) -> Self {
        Self {
            server: Some(server),
            monitoring_server: Some(monitoring_server),
            log_rotation: Some(log_rotation),
        }
    }

    /// Start the server and handle shutdown signals
    async fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Set up comprehensive signal handling
        let shutdown_signal = self.setup_signal_handlers().await;

        // Start log rotation manager
        if let Some(log_rotation) = &self.log_rotation {
            if let Err(e) = log_rotation.start().await {
                warn!("Failed to start log rotation manager: {}", e);
            } else {
                info!("Log rotation manager started");
            }
        }

        // Start monitoring server
        let monitoring_handle = self.monitoring_server.take().map(|monitoring_server| {
            tokio::spawn(async move {
                if let Err(e) = monitoring_server.start().await {
                    error!("Monitoring server error: {}", e);
                }
            })
        });

        // Start the main server
        let mut server = self.server.take().unwrap();

        info!("RustyPotato server starting up...");

        // Run server with shutdown handling
        tokio::select! {
            result = server.start() => {
                match result {
                    Ok(()) => {
                        info!("Server stopped normally");

                        // Stop monitoring server
                        if let Some(handle) = monitoring_handle {
                            handle.abort();
                        }

                        Ok(())
                    }
                    Err(e) => {
                        error!("Server error: {}", e);

                        // Stop monitoring server
                        if let Some(handle) = monitoring_handle {
                            handle.abort();
                        }

                        Err(e.into())
                    }
                }
            }
            _ = shutdown_signal => {
                info!("Shutdown signal received, initiating graceful shutdown...");

                // Stop monitoring server first
                if let Some(handle) = monitoring_handle {
                    handle.abort();
                    info!("Monitoring server stopped");
                }

                // Perform graceful shutdown with timeout
                let shutdown_timeout = Duration::from_secs(30);
                match tokio::time::timeout(shutdown_timeout, server.shutdown()).await {
                    Ok(Ok(())) => {
                        info!("Server shutdown completed successfully");
                        Ok(())
                    }
                    Ok(Err(e)) => {
                        error!("Error during server shutdown: {}", e);
                        Err(e.into())
                    }
                    Err(_) => {
                        error!("Server shutdown timed out after {:?}, forcing exit", shutdown_timeout);
                        process::exit(1);
                    }
                }
            }
        }
    }

    /// Set up comprehensive signal handling for graceful shutdown.
    ///
    /// Returns a future that resolves once any of the OS shutdown
    /// signals has been observed. The caller `select!`s this future
    /// against the running server task; when it resolves, the
    /// shutdown branch fires.
    async fn setup_signal_handlers(&self) -> impl std::future::Future<Output = ()> {
        async move {
            #[cfg(unix)]
            {
                use signal::{unix::signal, unix::SignalKind};

                let mut sigterm = signal(SignalKind::terminate())
                    .expect("Failed to install SIGTERM signal handler");
                let mut sigint = signal(SignalKind::interrupt())
                    .expect("Failed to install SIGINT signal handler");
                let mut sigquit =
                    signal(SignalKind::quit()).expect("Failed to install SIGQUIT signal handler");

                tokio::select! {
                    _ = sigterm.recv() => {
                        warn!("Received SIGTERM, initiating graceful shutdown...");
                    }
                    _ = sigint.recv() => {
                        warn!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
                    }
                    _ = sigquit.recv() => {
                        warn!("Received SIGQUIT, initiating graceful shutdown...");
                    }
                }
            }

            #[cfg(windows)]
            {
                match signal::ctrl_c().await {
                    Ok(()) => {
                        warn!("Received Ctrl+C, initiating graceful shutdown...");
                    }
                    Err(e) => {
                        error!("Failed to listen for Ctrl+C signal: {}", e);
                    }
                }
            }
        }
    }
}

/// Initialize logging with enhanced configuration.
///
/// Branches on `config.logging.format` so that `Json`/`Compact` settings
/// from `rustypotato.toml` actually take effect — until Stage 13's
/// audit landed they were parsed and validated but ignored, with the
/// default `Pretty` formatter always selected. The writer is chosen by
/// `file_path` (file or stderr) orthogonally to the formatter choice.
fn init_logging(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    // Open the log writer (file or stderr) once. The same writer is
    // handed to whichever formatter we end up using.
    let file_writer = match &config.logging.file_path {
        Some(file_path) => Some(Arc::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file_path)
                .map_err(|e| format!("Failed to open log file {}: {}", file_path.display(), e))?,
        )),
        None => None,
    };

    // Macro: build a `fmt()` subscriber, apply common options, install
    // the chosen writer, and try-init. Each format variant produces a
    // distinct concrete type so the body is duplicated three ways
    // rather than abstracted behind a trait.
    macro_rules! init_with {
        ($fmt_chain:expr) => {{
            let subscriber = $fmt_chain
                .with_env_filter(env_filter)
                .with_target(false)
                .with_thread_ids(true)
                .with_file(true)
                .with_line_number(true);

            let init_result = match &file_writer {
                Some(file) => subscriber.with_writer(Arc::clone(file)).try_init(),
                None => subscriber.try_init(),
            };
            match init_result {
                Ok(()) => match &config.logging.file_path {
                    Some(p) => info!("Logging initialized with file output: {}", p.display()),
                    None => info!("Logging initialized with console output"),
                },
                Err(_) => {
                    debug!("Logging subscriber already initialized; honoring existing config");
                }
            }
        }};
    }

    match config.logging.format {
        LogFormat::Json => init_with!(fmt().json()),
        LogFormat::Compact => init_with!(fmt().compact()),
        LogFormat::Pretty => init_with!(fmt().pretty()),
    }

    Ok(())
}

/// Validate system requirements and environment
fn validate_environment(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    // Check if we can bind to the specified address
    if config.server.port != 0 {
        let bind_addr = format!("{}:{}", config.server.bind_address, config.server.port);
        match std::net::TcpListener::bind(&bind_addr) {
            Ok(listener) => {
                drop(listener);
                debug!("Successfully validated bind address: {}", bind_addr);
            }
            Err(e) => {
                return Err(format!("Cannot bind to address {bind_addr}: {e}").into());
            }
        }
    }

    // Validate AOF directory if persistence is enabled. Two checks:
    //  1) the directory exists (or can be created), and
    //  2) it is actually writable. We open + immediately delete a probe
    //     file rather than just `parent.exists()` because a directory on a
    //     read-only mount or with restrictive ACLs can pass `exists()` and
    //     still fail every subsequent write.
    if config.storage.aof_enabled {
        if let Some(parent) = config.storage.aof_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!("Failed to create AOF directory {}: {}", parent.display(), e)
                })?;
                info!("Created AOF directory: {}", parent.display());
            }

            let probe = if parent.as_os_str().is_empty() {
                std::path::PathBuf::from(".rustypotato-aof-write-probe")
            } else {
                parent.join(".rustypotato-aof-write-probe")
            };
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&probe)
                .and_then(|_| std::fs::remove_file(&probe))
                .map_err(|e| {
                    format!(
                        "AOF directory {} is not writable: {e}",
                        if parent.as_os_str().is_empty() {
                            std::path::Path::new(".")
                        } else {
                            parent
                        }
                        .display()
                    )
                })?;
        }
    }

    Ok(())
}

/// Command line arguments for the server
#[derive(Debug)]
struct ServerArgs {
    config_file: Option<PathBuf>,
    port: Option<u16>,
    bind_address: Option<String>,
    log_level: Option<String>,
    version: bool,
}

/// Parse command line arguments
fn parse_args() -> ServerArgs {
    let matches = Command::new("rustypotato-server")
        .version(env!("CARGO_PKG_VERSION"))
        .author("RustyPotato Team")
        .about("High-performance Redis-compatible key-value store")
        .long_about("RustyPotato is a high-performance, Redis-compatible key-value store written in Rust. \
                    It provides excellent performance, memory safety, and compatibility with Redis clients.")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Configuration file path")
                .value_parser(clap::value_parser!(PathBuf))
        )
        .arg(
            Arg::new("port")
                .short('p')
                .long("port")
                .value_name("PORT")
                .help("Server port (overrides config file)")
                .value_parser(clap::value_parser!(u16))
        )
        .arg(
            Arg::new("bind")
                .short('b')
                .long("bind")
                .value_name("ADDRESS")
                .help("Bind address (overrides config file)")
                .value_parser(clap::value_parser!(String))
        )
        .arg(
            Arg::new("log-level")
                .short('l')
                .long("log-level")
                .value_name("LEVEL")
                .help("Log level: trace, debug, info, warn, error")
                .value_parser(["trace", "debug", "info", "warn", "error"])
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .help("Print version information")
                .action(clap::ArgAction::SetTrue)
        )
        .get_matches();

    ServerArgs {
        config_file: matches.get_one::<PathBuf>("config").cloned(),
        port: matches.get_one::<u16>("port").copied(),
        bind_address: matches.get_one::<String>("bind").cloned(),
        log_level: matches.get_one::<String>("log-level").cloned(),
        version: matches.get_flag("version"),
    }
}

/// Apply command line overrides to configuration
fn apply_cli_overrides(mut config: Config, args: &ServerArgs) -> Config {
    if let Some(port) = args.port {
        config.server.port = port;
        info!("Port overridden by CLI: {}", port);
    }

    if let Some(ref bind_address) = args.bind_address {
        config.server.bind_address = bind_address.clone();
        info!("Bind address overridden by CLI: {}", bind_address);
    }

    if let Some(ref log_level) = args.log_level {
        config.logging.level = log_level.clone();
        info!("Log level overridden by CLI: {}", log_level);
    }

    config
}

/// Display version information
fn display_version() {
    println!("RustyPotato Server v{}", env!("CARGO_PKG_VERSION"));
    println!("A high-performance Redis-compatible key-value store");
    println!();
    println!("Build Information:");
    println!("  Version: {}", env!("CARGO_PKG_VERSION"));
    println!("  Target: {}", std::env::consts::ARCH);
    println!(
        "  Profile: {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!();
    println!(
        "For more information, visit: {}",
        env!("CARGO_PKG_REPOSITORY")
    );
}

/// Display startup banner and system information
fn display_startup_info(config: &Config) {
    info!("╔══════════════════════════════════════════════════════════════╗");
    info!("║                        RustyPotato                           ║");
    info!("║              High-Performance Key-Value Store               ║");
    info!("╚══════════════════════════════════════════════════════════════╝");
    info!("");
    info!("Server Configuration:");
    info!(
        "  • Bind Address: {}:{}",
        config.server.bind_address, config.server.port
    );
    info!("  • Max Connections: {}", config.server.max_connections);
    info!(
        "  • Worker Threads: {}",
        config
            .server
            .worker_threads
            .map_or("auto".to_string(), |n| n.to_string())
    );
    info!("");
    info!("Storage Configuration:");
    info!("  • AOF Enabled: {}", config.storage.aof_enabled);
    if config.storage.aof_enabled {
        info!("  • AOF Path: {}", config.storage.aof_path.display());
        info!(
            "  • AOF Fsync Policy: {:?}",
            config.storage.aof_fsync_policy
        );
    }
    info!("");
    info!("Network Configuration:");
    info!("  • TCP NoDelay: {}", config.network.tcp_nodelay);
    info!("  • TCP KeepAlive: {}", config.network.tcp_keepalive);
    info!(
        "  • Read/Write Timeout: {}s/{}s",
        config.network.read_timeout, config.network.write_timeout
    );
    info!("  • Idle Timeout: {}s", config.network.idle_timeout);
    info!("");
}

/// Construct a tokio runtime, honoring `config.server.worker_threads`
/// when set. Without this, the previous `#[tokio::main]` attribute used
/// the default multi-thread runtime and silently ignored the config
/// value — operators saw "Worker Threads: 8" in the startup banner
/// while the runtime actually had `std::thread::available_parallelism()`
/// workers. Now the value takes effect.
fn build_runtime(config: &Config) -> std::io::Result<tokio::runtime::Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    if let Some(n) = config.server.worker_threads {
        if n > 0 {
            builder.worker_threads(n);
        }
    }
    builder.build()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments — this is sync work and shouldn't
    // need a runtime.
    let args = parse_args();

    // Handle version flag
    if args.version {
        display_version();
        return Ok(());
    }

    // Load configuration from file or default
    let config = if let Some(config_file) = &args.config_file {
        Config::load_from_file(Some(config_file)).map_err(|e| {
            eprintln!(
                "Failed to load configuration from {}: {}",
                config_file.display(),
                e
            );
            e
        })?
    } else {
        Config::load().map_err(|e| {
            eprintln!("Failed to load configuration: {e}");
            e
        })?
    };

    // Apply CLI overrides to configuration
    let config = apply_cli_overrides(config, &args);

    // Build the runtime BEFORE initializing tracing, so any early
    // tracing output emitted during async setup uses the configured
    // worker thread count consistently.
    let runtime = build_runtime(&config)?;
    runtime.block_on(async_main(config))
}

async fn async_main(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with configuration
    init_logging(&config)?;

    info!("RustyPotato server initializing...");

    // Display startup information
    display_startup_info(&config);

    // Validate environment and system requirements
    validate_environment(&config).map_err(|e| {
        error!("Environment validation failed: {}", e);
        e
    })?;

    // Build the server. `RustyPotatoServer::open` handles AOF recovery
    // and persistence wiring when `config.storage.aof_enabled` is true.
    let server = rustypotato::RustyPotatoServer::open(config.clone())
        .await
        .map_err(|e| {
            error!("Failed to create server instance: {}", e);
            e
        })?;
    let storage = server.storage_arc();
    let metrics = server.metrics_arc();

    // Set up log rotation manager
    let log_rotation_config = LogRotationConfig {
        log_file_path: config
            .logging
            .file_path
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("rustypotato.log")),
        rotation_policy: RotationPolicy::SizeOrDaily {
            size_bytes: 100 * 1024 * 1024, // 100MB
            hour: 0,                       // Midnight
        },
        max_files: 7,
        compress: true,
        rotation_dir: None,
    };
    let log_rotation = Arc::new(LogRotationManager::new(log_rotation_config));

    // Set up health checker and monitoring server
    let health_checker = std::sync::Arc::new(HealthChecker::new(
        std::sync::Arc::clone(&storage),
        std::sync::Arc::clone(&metrics),
    ));

    let monitoring_server = MonitoringServer::new(
        health_checker,
        std::sync::Arc::clone(&metrics),
        Arc::clone(&log_rotation),
        "127.0.0.1".to_string(),
        config.server.port + 1000, // Monitoring on port + 1000
    );

    // Display server statistics
    let stats = server.stats().await;
    info!("Server initialized successfully:");
    info!(
        "  • Registered Commands: {} ({:?})",
        stats.registered_commands, stats.command_names
    );
    info!("  • Configuration: {}", stats.config_summary);
    info!("  • Monitoring Port: {}", config.server.port + 1000);
    info!("");

    // Set up shutdown coordinator and run server
    let coordinator = ShutdownCoordinator::new(server, monitoring_server, log_rotation);

    match coordinator.run().await {
        Ok(()) => {
            info!("RustyPotato server shutdown complete");
            Ok(())
        }
        Err(e) => {
            error!("Server failed: {}", e);
            process::exit(1);
        }
    }
}
