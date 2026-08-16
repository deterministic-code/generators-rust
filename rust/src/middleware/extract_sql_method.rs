use std::collections::HashSet;
use std::sync::OnceLock;

const SCAN_CAP: usize = 4 * 1024;

fn terminal_verbs() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut s = HashSet::new();
        s.insert("SELECT");
        s.insert("INSERT");
        s.insert("UPDATE");
        s.insert("DELETE");
        s.insert("MERGE");
        s
    })
}

fn known_verbs() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut s = HashSet::new();
        for v in terminal_verbs() {
            s.insert(*v);
        }
        for v in [
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "PRAGMA",
            "EXPLAIN",
            "VACUUM",
            "CREATE",
            "DROP",
            "ALTER",
            "TRUNCATE",
            "SAVEPOINT",
            "RELEASE",
            "ATTACH",
            "DETACH",
            "ANALYZE",
            "REINDEX",
        ] {
            s.insert(v);
        }
        s
    })
}

fn skip_comments_and_whitespace(sql: &[u8], start: usize) -> usize {
    let mut i = start;
    let len = sql.len();
    while i < len {
        let ch = sql[i];
        if ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' {
            i += 1;
            continue;
        }
        if ch == b'-' && i + 1 < len && sql[i + 1] == b'-' {
            match memchr(b'\n', &sql[i..]) {
                Some(off) => i += off + 1,
                None => i = len,
            }
            continue;
        }
        if ch == b'/' && i + 1 < len && sql[i + 1] == b'*' {
            match find_block_close(sql, i + 2) {
                Some(end) => i = end + 2,
                None => i = len,
            }
            continue;
        }
        return i;
    }
    i
}

fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|b| *b == needle)
}

fn find_block_close(sql: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < sql.len() {
        if sql[i] == b'*' && sql[i + 1] == b'/' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_ascii_alpha(b: u8) -> bool {
    (b'A'..=b'Z').contains(&b) || (b'a'..=b'z').contains(&b)
}

fn read_identifier(sql: &[u8], start: usize) -> String {
    let mut i = start;
    while i < sql.len() && is_ascii_alpha(sql[i]) {
        i += 1;
    }
    std::str::from_utf8(&sql[start..i])
        .unwrap_or("")
        .to_ascii_uppercase()
}

pub fn extract_sql_method(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let scan_len = bytes.len().min(SCAN_CAP);
    if scan_len == 0 {
        return "OTHER".to_string();
    }

    let mut i = skip_comments_and_whitespace(bytes, 0);
    if i >= scan_len {
        return "OTHER".to_string();
    }

    let first = read_identifier(bytes, i);
    if first.is_empty() {
        return "OTHER".to_string();
    }
    if first != "WITH" {
        return if known_verbs().contains(first.as_str()) {
            first
        } else {
            "OTHER".to_string()
        };
    }

    i += first.len();
    let mut depth: usize = 0;
    while i < scan_len {
        let ch = bytes[i];
        if ch == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            match memchr(b'\n', &bytes[i..scan_len.min(bytes.len())]) {
                Some(off) => i += off + 1,
                None => i = scan_len,
            }
            continue;
        }
        if ch == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            match find_block_close(&bytes[..scan_len.min(bytes.len())], i + 2) {
                Some(end) => i = end + 2,
                None => i = scan_len,
            }
            continue;
        }
        if ch == b'(' {
            depth += 1;
            i += 1;
            continue;
        }
        if ch == b')' {
            if depth > 0 {
                depth -= 1;
            }
            i += 1;
            continue;
        }
        if depth == 0 && is_ascii_alpha(ch) {
            let word = read_identifier(bytes, i);
            if terminal_verbs().contains(word.as_str()) {
                return word;
            }
            i += word.len();
            continue;
        }
        i += 1;
    }
    "OTHER".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cases() -> Vec<(&'static str, &'static str, &'static str)> {
        vec![
            ("plain SELECT", "SELECT * FROM users", "SELECT"),
            ("leading whitespace", "   SELECT 1", "SELECT"),
            ("lower-case", "select 1", "SELECT"),
            ("mixed case", "SeLeCt 1", "SELECT"),
            ("INSERT", "INSERT INTO t VALUES (1)", "INSERT"),
            ("UPDATE", "UPDATE t SET a = 1", "UPDATE"),
            ("DELETE", "DELETE FROM t WHERE id = 1", "DELETE"),
            (
                "MERGE",
                "MERGE INTO t USING s ON 1=1 WHEN MATCHED THEN UPDATE",
                "MERGE",
            ),
            ("BEGIN tx", "BEGIN", "BEGIN"),
            ("COMMIT tx", "COMMIT", "COMMIT"),
            ("PRAGMA (sqlite)", "PRAGMA foreign_keys = ON", "PRAGMA"),
            (
                "line comment then SELECT",
                "-- pick rows\nSELECT 1",
                "SELECT",
            ),
            (
                "block comment then INSERT",
                "/* hi */ INSERT INTO t VALUES (1)",
                "INSERT",
            ),
            (
                "mixed comments then UPDATE",
                "-- a\n/* b */ -- c\n  UPDATE t SET x = 1",
                "UPDATE",
            ),
            (
                "WITH cte then SELECT",
                "WITH x AS (SELECT 1) SELECT * FROM x",
                "SELECT",
            ),
            (
                "WITH cte then INSERT",
                "WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x",
                "INSERT",
            ),
            (
                "WITH cte then UPDATE",
                "WITH x AS (SELECT 1) UPDATE t SET a = (SELECT 1 FROM x)",
                "UPDATE",
            ),
            (
                "WITH cte then DELETE",
                "WITH x AS (SELECT 1) DELETE FROM t WHERE id IN (SELECT 1 FROM x)",
                "DELETE",
            ),
            (
                "WITH lower-case then SELECT",
                "with x as (select 1) select * from x",
                "SELECT",
            ),
            (
                "nested parens before terminal verb",
                "WITH x AS (SELECT (1)) SELECT 1",
                "SELECT",
            ),
            ("unknown verb", "FOOBAR 1", "OTHER"),
            ("empty input", "", "OTHER"),
            ("only comments", "-- nothing\n/* still nothing */", "OTHER"),
        ]
    }

    #[test]
    fn extracts_select_from_plain_select() {
        let (_n, sql, expected) = cases()[0];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_select_from_leading_whitespace() {
        let (_n, sql, expected) = cases()[1];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_select_from_lower_case() {
        let (_n, sql, expected) = cases()[2];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_select_from_mixed_case() {
        let (_n, sql, expected) = cases()[3];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_insert() {
        let (_n, sql, expected) = cases()[4];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_update() {
        let (_n, sql, expected) = cases()[5];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_delete() {
        let (_n, sql, expected) = cases()[6];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_merge() {
        let (_n, sql, expected) = cases()[7];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_begin_tx() {
        let (_n, sql, expected) = cases()[8];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_commit_tx() {
        let (_n, sql, expected) = cases()[9];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_pragma_sqlite() {
        let (_n, sql, expected) = cases()[10];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_select_from_line_comment_then_select() {
        let (_n, sql, expected) = cases()[11];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_insert_from_block_comment_then_insert() {
        let (_n, sql, expected) = cases()[12];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_update_from_mixed_comments_then_update() {
        let (_n, sql, expected) = cases()[13];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_select_from_with_cte_then_select() {
        let (_n, sql, expected) = cases()[14];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_insert_from_with_cte_then_insert() {
        let (_n, sql, expected) = cases()[15];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_update_from_with_cte_then_update() {
        let (_n, sql, expected) = cases()[16];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_delete_from_with_cte_then_delete() {
        let (_n, sql, expected) = cases()[17];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_select_from_with_lower_case_then_select() {
        let (_n, sql, expected) = cases()[18];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_select_from_nested_parens_before_terminal_verb() {
        let (_n, sql, expected) = cases()[19];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_other_from_unknown_verb() {
        let (_n, sql, expected) = cases()[20];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_other_from_empty_input() {
        let (_n, sql, expected) = cases()[21];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn extracts_other_from_only_comments() {
        let (_n, sql, expected) = cases()[22];
        assert_eq!(extract_sql_method(sql), expected);
    }

    #[test]
    fn caps_scan_length_so_pathological_input_does_not_hang() {
        let huge = format!("/*{}*/ SELECT 1", "x".repeat(10_000));
        assert_eq!(extract_sql_method(&huge), "OTHER");
    }
}
