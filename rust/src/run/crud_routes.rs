pub fn pluralize_table_name(entity: &str) -> String {
    pluralize_last_word(entity)
}

// Mirrors scripts/lib/routes-expand.mjs::kebabPlural — replace _ with -, then pluralize the last hyphen-separated segment. Keeps DB table identifiers snake_case while URL path segments become kebab-case (`notification_type` → `notification-types`).
pub fn pluralize_path_segment(entity: &str) -> String {
    let kebab = entity.replace('_', "-");
    let mut parts: Vec<String> = kebab.split('-').map(|s| s.to_string()).collect();
    if let Some(last) = parts.pop() {
        parts.push(pluralize_last_word(&last));
    }
    parts.join("-")
}

fn pluralize_last_word(entity: &str) -> String {
    if entity.ends_with('y')
        && entity.len() >= 2
        && !is_vowel(entity.as_bytes()[entity.len() - 2] as char)
    {
        let mut out = entity[..entity.len() - 1].to_string();
        out.push_str("ies");
        out
    } else if entity.ends_with('s')
        || entity.ends_with("sh")
        || entity.ends_with("ch")
        || entity.ends_with('x')
        || entity.ends_with('z')
    {
        format!("{}es", entity)
    } else {
        format!("{}s", entity)
    }
}

fn is_vowel(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pluralize_y_ies() {
        assert_eq!(pluralize_table_name("notification"), "notifications");
        assert_eq!(pluralize_table_name("category"), "categories");
        assert_eq!(pluralize_table_name("party"), "parties");
        assert_eq!(pluralize_table_name("guy"), "guys");
        assert_eq!(pluralize_table_name("address"), "addresses");
        assert_eq!(pluralize_table_name("box"), "boxes");
    }

    #[test]
    fn pluralize_path_segment_mirrors_kebab_plural() {
        assert_eq!(
            pluralize_path_segment("notification_type"),
            "notification-types"
        );
        assert_eq!(pluralize_path_segment("event_log"), "event-logs");
        assert_eq!(pluralize_path_segment("category"), "categories");
        assert_eq!(pluralize_path_segment("widget"), "widgets");
    }

    #[test]
    fn pluralize_table_name_stays_snake() {
        assert_eq!(
            pluralize_table_name("notification_type"),
            "notification_types"
        );
        assert_eq!(pluralize_table_name("event_log"), "event_logs");
    }
}
