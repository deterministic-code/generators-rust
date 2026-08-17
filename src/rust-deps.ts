// App-crate deps: exactly the crates generated code references (types use chrono/uuid, services use async-trait/serde_json, main uses tokio/axum). sqlx/dotenvy ride the MIGRATE_DEPS marker so the migrate step owns SQL coupling.
export const RUST_APP_DEPS = `async-trait = "0.1"
axum = "0.8"
chrono = { version = "0.4", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
time = "=0.3.47"
tokio = { version = "1", features = ["full"] }
uuid = { version = "1.10", features = ["v4"] }
`;

export const RUST_APP_DEV_DEPS = `http-body-util = "0.1"
jsonschema = "0.18"
once_cell = "1"
reqwest = { version = "0.12", features = ["json", "blocking"] }
serde_yaml = "0.9"
tower = { version = "0.5", features = ["util"] }
`;
