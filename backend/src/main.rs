use mcap_workbench::{router, Store};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mcap_workbench=info,tower_http=info".into()),
        )
        .init();
    let workspace = std::env::var_os("MCAP_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_owned()
        });
    let host = std::env::var("MCAP_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = std::env::var("MCAP_BACKEND_PORT").unwrap_or_else(|_| "8000".to_owned());
    let address: SocketAddr = std::env::var("MCAP_BIND")
        .unwrap_or_else(|_| format!("{host}:{port}"))
        .parse()?;
    let store = Arc::new(Store::new(&workspace)?);
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, workspace = %workspace.display(), "MCAP Workbench backend ready");
    axum::serve(listener, router(store))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
