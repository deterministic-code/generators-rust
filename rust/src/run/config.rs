use std::path::PathBuf;

use super::compose::RouteComposer;
use crate::services::CustomServices;

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub deterministic_dir: PathBuf,
    pub database_url: String,
    pub port: u16,
    pub custom_services: CustomServices,
    /// When set (by the generated backend's `main.rs`), the runtime routes requests through the
    /// generated routes → services this composer wires, instead of rebuilding CRUD from YAML.
    pub route_composer: Option<RouteComposer>,
}

impl RunConfig {
    pub fn from_env() -> Self {
        // from_path (not dotenv()): dotenv() walks parent directories and could pick up an unrelated .env; the app only honors its own cwd's file, matching node's --env-file-if-exists.
        require_dotenv_loaded(dotenvy::from_path(".env"));
        let deterministic_dir = std::env::var("DETERMINISTIC_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./deterministic"));
        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".to_string());
        // 4002 is the rust-lane dev default: 4000 is the deterministic dev server and 4001 the typescript lane, so all three can run at once (see DEV_PORTS in scripts/codegen/lib/create-backend-app-model.mjs).
        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4002);
        Self {
            deterministic_dir,
            database_url,
            port,
            custom_services: CustomServices::new(),
            route_composer: None,
        }
    }
}

// Missing .env is the normal case (docker sets real env vars); anything else — malformed line, permission error — panics so a broken .env can't silently boot the app with half its config.
fn require_dotenv_loaded(result: Result<(), dotenvy::Error>) {
    match result {
        Ok(_) => {}
        Err(dotenvy::Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => panic!("failed to load .env: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_dotenv_loaded_accepts_a_loaded_file() {
        require_dotenv_loaded(Ok(()));
    }

    #[test]
    fn require_dotenv_loaded_accepts_a_missing_file() {
        require_dotenv_loaded(Err(dotenvy::Error::Io(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        ))));
    }

    #[test]
    #[should_panic(expected = "failed to load .env")]
    fn require_dotenv_loaded_panics_on_a_malformed_file() {
        require_dotenv_loaded(Err(dotenvy::Error::LineParse("PORT==oops".to_string(), 1)));
    }

    #[test]
    #[should_panic(expected = "failed to load .env")]
    fn require_dotenv_loaded_panics_on_a_permission_error() {
        require_dotenv_loaded(Err(dotenvy::Error::Io(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        ))));
    }
}
