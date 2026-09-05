mod domains;
mod handlers;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::SmartIpKeyExtractor;
use tower_governor::GovernorLayer;
use tower_http::services::{ServeDir, ServeFile};

use state::{AppState, Config};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = Config::from_env();
    std::fs::create_dir_all(&config.download_dir)
        .expect("não foi possível criar o diretório de downloads");

    let port = config.port;
    let frontend_dist = config.frontend_dist.clone();
    let rate_limit_per_minute = config.rate_limit_per_minute.max(1);

    let state = Arc::new(AppState::new(config));
    handlers::spawn_cleanup_task(Arc::clone(&state));

    let period_secs = (60 / rate_limit_per_minute).max(1);
    let mut builder = GovernorConfigBuilder::default();
    let mut builder = builder.key_extractor(SmartIpKeyExtractor);
    let governor_conf = Arc::new(
        builder
            .per_second(period_secs)
            .burst_size(rate_limit_per_minute as u32)
            .finish()
            .expect("configuração de rate limit inválida"),
    );

    // Rotas com rate limiting por IP (fetch de formatos e início de download).
    let limited = Router::new()
        .route("/formats", post(handlers::formats_handler))
        .route("/downloads", post(handlers::start_download_handler))
        .layer(GovernorLayer {
            config: governor_conf,
        });

    // Rotas sem rate limiting: cancelamento, download do arquivo já pronto e o
    // websocket de progresso (uma única conexão de longa duração por aba).
    let unlimited = Router::new()
        .route("/downloads/:id/cancel", post(handlers::cancel_handler))
        .route("/downloads/:id/file", get(handlers::file_handler))
        .route("/ws", get(handlers::ws_handler));

    let api = limited.merge(unlimited).with_state(state);

    let index_html = frontend_dist.join("index.html");
    let app = Router::new().nest("/api", api).fallback_service(
        ServeDir::new(&frontend_dist).not_found_service(ServeFile::new(index_html)),
    );

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("mptube-server ouvindo em {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("não foi possível abrir a porta {addr}: {e}"));

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
