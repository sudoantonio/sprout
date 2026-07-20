use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sprout_server::{
    AppState, build_router,
    config::Config,
    worker::{self, WorkerKind, WorkerOptions},
};
use sprout_storage_postgres::PostgresStorage;
use tokio::{net::TcpListener, sync::watch};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Parser)]
#[command(name = "sprout-server", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        skip_migrations: bool,
    },
    Worker {
        #[arg(long, value_enum, default_value_t = WorkerKind::All)]
        kind: WorkerKind,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        once: bool,
        #[arg(long, default_value_t = 30)]
        interval_seconds: u64,
        #[arg(long, default_value_t = 120)]
        lease_ttl_seconds: i64,
        #[arg(long)]
        skip_migrations: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    init_tracing()?;
    let cli = Cli::parse();
    let config = Config::from_env().context("invalid server configuration")?;
    tracing::info!(config = ?config, "configuration loaded");
    let storage = PostgresStorage::connect(
        config.database_url.expose(),
        config.database_max_connections,
    )
    .await
    .context("failed to connect to PostgreSQL")?;

    match cli.command.unwrap_or(Command::Serve {
        skip_migrations: false,
    }) {
        Command::Serve { skip_migrations } => {
            migrate_if_enabled(&storage, &config, skip_migrations).await?;
            serve(config, storage).await
        }
        Command::Worker {
            kind,
            dry_run,
            once,
            interval_seconds,
            lease_ttl_seconds,
            skip_migrations,
        } => {
            migrate_if_enabled(&storage, &config, skip_migrations).await?;
            if interval_seconds == 0 || !(1..=86_400).contains(&lease_ttl_seconds) {
                anyhow::bail!("worker interval and lease TTL must be positive and bounded");
            }
            run_worker(
                storage,
                config,
                WorkerOptions {
                    kind,
                    dry_run,
                    once,
                    interval: Duration::from_secs(interval_seconds),
                    lease_ttl_seconds,
                },
            )
            .await
        }
    }
}

async fn migrate_if_enabled(storage: &PostgresStorage, config: &Config, skip: bool) -> Result<()> {
    if skip {
        tracing::warn!("database migrations were explicitly skipped");
        return Ok(());
    }
    storage
        .migrate(&config.migrations_dir)
        .await
        .context("database migrations failed")
}

async fn serve(config: Config, storage: PostgresStorage) -> Result<()> {
    let listener = TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.bind_addr))?;
    let local_address = listener
        .local_addr()
        .context("listener has no local address")?;
    let state = Arc::new(
        AppState::new(config, storage.pool().clone())
            .context("failed to initialize application")?,
    );
    let app = build_router(state).context("failed to construct router")?;
    tracing::info!(bind_addr = %local_address, "Sprout HTTP server listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("HTTP server failed")
}

async fn run_worker(
    storage: PostgresStorage,
    config: Config,
    options: WorkerOptions,
) -> Result<()> {
    tracing::warn!(
        "workers require a dedicated PostgreSQL role with BYPASSRLS; never expose that role to HTTP requests"
    );
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let worker = worker::run(storage.pool().clone(), config, options, shutdown_receiver);
    tokio::pin!(worker);
    tokio::select! {
        result = &mut worker => result.context("worker failed"),
        () = shutdown_signal() => {
            let _ = shutdown_sender.send(true);
            worker.await.context("worker failed during shutdown")
        }
    }
}

async fn shutdown_signal() {
    let control_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = control_c => {}
        () = terminate => {}
    }
    tracing::info!("shutdown requested");
}

fn init_tracing() -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("sprout_server=info,tower_http=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(true)
                .with_span_list(false),
        )
        .try_init()
        .context("failed to initialize structured logging")
}
