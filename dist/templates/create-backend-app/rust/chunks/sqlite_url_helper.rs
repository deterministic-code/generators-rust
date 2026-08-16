fn sqlite_url(connection: &str) -> String {
    if connection.starts_with("sqlite:") {
        return connection.to_string();
    }
    let path = PathBuf::from(connection);
    let abs = if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    };
    format!("sqlite://{}?mode=rwc", abs.display())
}
