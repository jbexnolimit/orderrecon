use orderrecon::{config, database, routes};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("orderrecon=debug")),
        )
        .init();

    let config = config::Config::from_env()?;

    let database = database::connect(&config.mongodb_uri, &config.mongodb_database).await?;

    database::ensure_indexes(&database).await?;

    tracing::info!("MongoDB indexes initialized");

    tracing::info!(
        database = %config.mongodb_database,
        "Connected to MongoDB"
    );

    let state = routes::AppState { database };

    let address = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&address).await?;

    tracing::info!(address = %address, "OrderRecon API is running");

    axum::serve(listener, routes::create_router(state)).await?;

    Ok(())
}
