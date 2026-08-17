#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut config = deterministic::RunConfig::from_env();
    config.custom_services = {{crateIdent}}::custom_services();
    config.route_composer = Some({{crateIdent}}::route_composer());
    deterministic::run(config).await?;
    Ok(())
}
