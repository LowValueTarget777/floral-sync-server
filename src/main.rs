use axum::{
    routing::{get, post},
    Router,
};
use clap::{ArgAction, Args, Parser, Subcommand};
use floral_sync_server::{
    config::{
        default_config_path, generate_token, load_or_create_config, update_config_file,
        ConfigOverrides, ConfigPatch, RuntimeConfig, ServerConfig,
    },
    store::{StoreError, SyncStore},
    sync_api::{changes, health, push, wait_for_change, AppState},
};
#[cfg(feature = "admin")]
use floral_sync_server::{
    admin_api::{router as admin_router, AdminAppState, RestartHandle},
    store::AdminStore,
};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    process,
};
use thiserror::Error;
use tokio::{
    net::TcpListener,
    task::JoinSet,
};
use tower_http::trace::TraceLayer;

#[derive(Debug, Parser)]
#[command(
    name = "floral-sync-server",
    about = "Lightweight single-user sync server for Floral Notepaper"
)]
struct Cli {
    #[arg(long, value_name = "PATH", global = true)]
    config: Option<PathBuf>,
    #[arg(
        long = "listen",
        alias = "bind",
        value_name = "ADDR",
        action = ArgAction::Append
    )]
    listen: Vec<String>,
    #[arg(long = "db", value_name = "PATH")]
    db_path: Option<PathBuf>,
    #[arg(long, value_name = "TOKEN")]
    token: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Config(ConfigArgs),
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Show,
    Set(SetArgs),
}

#[derive(Debug, Args)]
struct SetArgs {
    #[arg(
        long = "listen",
        alias = "bind",
        value_name = "ADDR",
        action = ArgAction::Append
    )]
    listen: Vec<String>,
    #[arg(long = "db", value_name = "PATH")]
    db_path: Option<PathBuf>,
    #[arg(long, value_name = "TOKEN", conflicts_with = "generate_token")]
    token: Option<String>,
    #[arg(long)]
    generate_token: bool,
}

#[derive(Debug, Error)]
enum MainError {
    #[error("{0}")]
    Config(#[from] floral_sync_server::config::ConfigError),
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("invalid listen address: {0}")]
    AddrParse(#[from] std::net::AddrParseError),
    #[error("{0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("a listener stopped unexpectedly")]
    ListenerStopped,
    #[cfg(feature = "admin")]
    #[error("restart requested")]
    RestartRequested,
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(()) => {}
        #[cfg(feature = "admin")]
        Err(MainError::RestartRequested) => {
            println!("Restart requested; shutting down so the supervisor can start a fresh instance.");
            process::exit(0);
        }
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}

async fn run() -> Result<(), MainError> {
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or(default_config_path()?);

    match cli.command {
        Some(Command::Config(args)) => handle_config_command(&config_path, args),
        None => {
            let created_config = !config_path.exists();
            let config = load_or_create_config(
                &config_path,
                &ConfigOverrides {
                    listen: to_optional_listen(&cli.listen),
                    db_path: cli.db_path,
                    token: cli.token,
                },
            )?;
            if created_config {
                println!("Created config file at {}", config.config_path.display());
            }
            serve(config).await
        }
    }
}

fn handle_config_command(config_path: &Path, args: ConfigArgs) -> Result<(), MainError> {
    match args.command {
        ConfigCommand::Show => {
            let created_config = !config_path.exists();
            let config = load_or_create_config(config_path, &ConfigOverrides::default())?;
            if created_config {
                println!("Created config file at {}", config.config_path.display());
            } else {
                println!("Config file: {}", config.config_path.display());
            }
            print!("{}", fs::read_to_string(config_path)?);
            Ok(())
        }
        ConfigCommand::Set(args) => {
            let token = if args.generate_token {
                Some(generate_token())
            } else {
                args.token
            };
            let config = update_config_file(
                config_path,
                &ConfigPatch {
                    listen: to_optional_listen(&args.listen),
                    admin_listen: None,
                    db_path: args.db_path,
                    export_dir: None,
                    log_path: None,
                    log_level: None,
                    token,
                    admin_password_hash: None,
                },
            )?;
            println!("Updated config file at {}", config.config_path.display());
            print!("{}", fs::read_to_string(config_path)?);
            Ok(())
        }
    }
}

async fn serve(config: ServerConfig) -> Result<(), MainError> {
    let sync_store = SyncStore::open(&config.db_path)?;
    let runtime_config = RuntimeConfig::new(config.clone());
    #[cfg(feature = "admin")]
    let admin_store = AdminStore::open_shared(sync_store.connection())?;
    #[cfg(feature = "admin")]
    let restart_handle = RestartHandle::new();

    let sync_state = AppState::new(sync_store.clone(), runtime_config.clone());
    let sync_app = Router::new()
        .route("/health", get(health))
        .route("/v1/wait", get(wait_for_change))
        .route("/v1/changes", get(changes))
        .route("/v1/push", post(push))
        .layer(TraceLayer::new_for_http())
        .with_state(sync_state);
    #[cfg(feature = "admin")]
    let admin_app = admin_router(AdminAppState::new(
        sync_store,
        admin_store,
        runtime_config,
        restart_handle.clone(),
    ))
    .layer(TraceLayer::new_for_http());

    let sync_listeners = bind_listeners(&config.sync_listen)?;
    #[cfg(feature = "admin")]
    let admin_listeners = bind_listeners(&config.admin_listen)?;
    println!("Config: {}", config.config_path.display());
    println!("Database: {}", config.db_path.display());
    println!("Sync token: {}", config.sync_token);
    for address in &config.sync_listen {
        println!("Sync listening on {address}");
    }
    #[cfg(feature = "admin")]
    for address in &config.admin_listen {
        println!("Admin listening on {address}");
    }
    #[cfg(not(feature = "admin"))]
    println!("Admin UI: disabled in this build");

    let mut join_set = JoinSet::new();
    for listener in sync_listeners {
        let app = sync_app.clone();
        #[cfg(feature = "admin")]
        {
            let restart = restart_handle.clone();
            join_set.spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        restart.wait_for_restart().await;
                    })
                    .await
                    .map_err(MainError::from)
            });
        }
        #[cfg(not(feature = "admin"))]
        {
            join_set.spawn(async move { axum::serve(listener, app).await.map_err(MainError::from) });
        }
    }
    #[cfg(feature = "admin")]
    for listener in admin_listeners {
        let app = admin_app.clone();
        let restart = restart_handle.clone();
        join_set.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    restart.wait_for_restart().await;
                })
                .await
                .map_err(MainError::from)
        });
    }

    while let Some(result) = join_set.join_next().await {
        result??;
        #[cfg(feature = "admin")]
        if !restart_handle.is_requested() {
            return Err(MainError::ListenerStopped);
        }
        #[cfg(not(feature = "admin"))]
        return Err(MainError::ListenerStopped);
    }

    #[cfg(feature = "admin")]
    if restart_handle.is_requested() {
        Err(MainError::RestartRequested)
    } else {
        Err(MainError::ListenerStopped)
    }
    #[cfg(not(feature = "admin"))]
    {
        Err(MainError::ListenerStopped)
    }
}

fn bind_listeners(addresses: &[String]) -> Result<Vec<TcpListener>, MainError> {
    let mut listeners = Vec::with_capacity(addresses.len());
    for address in addresses {
        let socket_addr: SocketAddr = address.parse()?;
        listeners.push(bind_listener(socket_addr)?);
    }
    Ok(listeners)
}

fn bind_listener(address: SocketAddr) -> Result<TcpListener, MainError> {
    let socket = match address {
        SocketAddr::V4(_) => Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?,
        SocketAddr::V6(_) => {
            let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))?;
            // When both IPv4 and IPv6 listeners are configured, forcing the IPv6 socket
            // into v6-only mode avoids relying on platform-specific dual-stack defaults.
            socket.set_only_v6(true)?;
            socket
        }
    };
    socket.set_reuse_address(true)?;
    socket.bind(&address.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    let listener: std::net::TcpListener = socket.into();
    Ok(TcpListener::from_std(listener)?)
}

fn to_optional_listen(addresses: &[String]) -> Option<Vec<String>> {
    if addresses.is_empty() {
        None
    } else {
        Some(addresses.to_vec())
    }
}
