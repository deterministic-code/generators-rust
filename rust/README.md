# deterministic (Rust)

Rust HTTP runtime for generated apps: routes, middleware, YAML loaders,
and SQL identifier helpers.

Persistence lives in Datasources-generated repositories. This crate no
longer ships dialect CRUD repositories or field converters.

## Develop

```bash
cd rust
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
