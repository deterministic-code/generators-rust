# deterministic (Rust)

Rust port of the `repositories` layer from the
[deterministic](../README.md) project.

This crate mirrors the canonical TypeScript surface in
[`typescript/src/repositories/`](../typescript/src/repositories) with
idiomatic Rust naming:

- Interfaces are traits without the `I` prefix
  (`Repository`, `CrudRepository`, `StandardCrudRepository`,
  `Datasource`, `Setup`).
- Methods use `snake_case`: `query`, `find`, `find_all`, `find_by`,
  `add`, `update`, `delete`.
- Errors flow through a single `RepositoryError` enum.
- Async traits use the `async-trait` crate.

Backends:

- `inmemory` — pure in-memory implementations.
- `sqlite`, `postgres`, `mysql` — backed by `sqlx`.
- `sqlserver` — SQL-string generation only (live driver TBD).
- `oracle` — stub returning `RepositoryError::Unimplemented`
  (no production-grade async Oracle driver in the Rust ecosystem).

## Develop

```bash
cd rust
cargo build --all-features
cargo test --all-features
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
