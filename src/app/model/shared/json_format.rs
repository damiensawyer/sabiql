//! JSON column detection and pretty-printing for result cells.
//!
//! When the user enables "Format JSON" in the Wrapped Cell settings, columns
//! whose values are JSON objects or arrays are pretty-printed on display,
//! mirroring PostgreSQL's `jsonb_pretty`: 4-space indentation, one key per
//! line, scalars rendered inline.
//!
//! This module is pure and lives in the app layer (which already depends on
//! `serde_json`); the UI layer calls it during render.

use std::borrow::Cow;

use serde::Serialize as _;
use serde_json::Value;

/// Maximum number of rows sampled when deciding whether a column is JSON.
/// Keeps detection cheap on large result sets — a column is either JSON or it
/// isn't, and the first few rows are representative.
const DETECTION_SAMPLE_ROWS: usize = 50;

/// True when `s` parses as a JSON object or array.
///
/// Scalars (numbers, strings, booleans, `null`) are intentionally excluded:
/// pretty-printing them is pointless, and excluding them stops a numeric or
/// text column from being mistaken for JSON.
#[must_use]
pub fn is_json_object_or_array(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    matches!(
        serde_json::from_str::<Value>(trimmed),
        Ok(Value::Object(_) | Value::Array(_))
    )
}

/// Detect which columns hold JSON object/array values.
///
/// A column counts as JSON when every sampled non-empty cell parses as a JSON
/// object or array. Empty cells (e.g. SQL `NULL`) are skipped so a jsonb
/// column with some nulls still formats. A column with no sampled values at
/// all is treated as non-JSON (there is nothing to base a decision on).
///
/// `cell(row, col)` returns the display text of a cell, or `None` for SQL
/// `NULL` / missing cells. `num_columns` sets the result length so callers can
/// zip it with headers even when rows are ragged.
#[must_use]
pub fn detect_json_columns<'a>(
    row_count: usize,
    num_columns: usize,
    cell: impl Fn(usize, usize) -> Option<Cow<'a, str>>,
) -> Vec<bool> {
    let mut flags = vec![false; num_columns];

    let sample = row_count.min(DETECTION_SAMPLE_ROWS);
    if sample == 0 {
        return flags;
    }

    for (col, flag) in flags.iter_mut().enumerate() {
        let mut seen_any = false;
        let mut all_json = true;
        for row in 0..sample {
            let Some(cell) = cell(row, col) else {
                continue;
            };
            if cell.trim().is_empty() {
                continue;
            }
            seen_any = true;
            if !is_json_object_or_array(&cell) {
                all_json = false;
                break;
            }
        }
        *flag = seen_any && all_json;
    }

    flags
}

/// Pretty-print `s` as JSON with `jsonb_pretty`-style 4-space indentation.
///
/// If the compact representation fits within `max_width` characters, the
/// compact form is returned instead. Pass `0` to always use pretty.
///
/// Invalid JSON falls back to the original string unchanged, so a stray
/// non-JSON value in a detected column is never mangled.
#[must_use]
pub fn pretty_format_json(s: &str, max_width: usize) -> String {
    let trimmed = s.trim();
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return s.to_string();
    };
    let compact = value.to_string();
    if compact.len() <= max_width {
        return compact;
    }
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value
        .serialize(&mut ser)
        .expect("serializing an already-parsed Value is infallible");
    String::from_utf8(buf).expect("serde_json output is always valid UTF-8")
}

/// Return the display text for a cell, pretty-printing it when the column is
/// JSON and formatting is enabled. Non-JSON cells borrow the input with no
/// allocation.
#[must_use]
pub fn format_cell(val: &str, is_json_column: bool, format_json: bool) -> Cow<'_, str> {
    if format_json && is_json_column {
        Cow::Owned(pretty_format_json(val, 0))
    } else {
        Cow::Borrowed(val)
    }
}

/// Display-column index where the value starts on a JSON key-value line of
/// the form `<spaces>"key": <value>`, or 0 if the line doesn't match.
///
/// Used as a hanging indent when wrapping, so continuation lines of a long
/// value align under the value rather than column 0.
#[must_use]
pub fn json_key_value_hang(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut i = 0;

    // serde_json pretty-prints with ASCII spaces only.
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    let leading = i;

    if bytes.get(i) != Some(&b'"') {
        return 0;
    }
    i += 1;

    loop {
        match bytes.get(i) {
            Some(&b'\\') => i += 2,
            Some(&b'"') => {
                i += 1;
                break;
            }
            Some(_) => i += 1,
            None => return 0,
        }
    }

    if bytes.get(i..i + 2) != Some(b": ") {
        return 0;
    }
    i += 2;

    if i <= leading {
        return 0;
    }
    i
}

/// Hanging indent to use when wrapping `line` at `width` cells. Falls back to
/// 0 when the indent would leave fewer than two cells for content.
#[must_use]
pub fn wrap_hang(line: &str, width: usize) -> usize {
    let hang = json_key_value_hang(line);
    if hang + 2 > width { 0 } else { hang }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    mod json_key_value_hang_tests {
        use super::*;

        #[test]
        fn detects_simple_key() {
            assert_eq!(json_key_value_hang(r#"    "key": value"#), 11);
        }

        #[test]
        fn detects_nested_indent() {
            assert_eq!(json_key_value_hang(r#"        "deep": value"#), 16);
        }

        #[test]
        fn no_hang_for_plain_text() {
            assert_eq!(json_key_value_hang("hello world"), 0);
        }

        #[test]
        fn no_hang_for_object_open() {
            assert_eq!(json_key_value_hang(r#"    "obj": {"#), 11);
        }

        #[test]
        fn handles_escaped_quote_in_key() {
            // Key contains `\"` — hang should still land after `: `.
            assert_eq!(json_key_value_hang(r#"    "a\"b": value"#), 12);
        }

        #[test]
        fn no_hang_for_array_delimiter() {
            assert_eq!(json_key_value_hang("{"), 0);
            assert_eq!(json_key_value_hang("}"), 0);
        }
    }

    mod is_json_object_or_array {
        use super::*;

        #[test]
        fn detects_objects_and_arrays() {
            assert!(is_json_object_or_array(r#"{"a":1}"#));
            assert!(is_json_object_or_array("[1, 2, 3]"));
        }

        #[test]
        fn rejects_scalars() {
            assert!(!is_json_object_or_array("42"));
            assert!(!is_json_object_or_array(r#""hello""#));
            assert!(!is_json_object_or_array("true"));
            assert!(!is_json_object_or_array("null"));
        }

        #[test]
        fn rejects_empty_and_non_json() {
            assert!(!is_json_object_or_array(""));
            assert!(!is_json_object_or_array("   "));
            assert!(!is_json_object_or_array("not json"));
            assert!(!is_json_object_or_array("{bad}"));
        }

        #[test]
        fn trims_surrounding_whitespace() {
            assert!(is_json_object_or_array("  {\"a\":1}  \n"));
        }
    }

    mod detect_json_columns {
        use super::*;

        fn detect(rows: &[Vec<String>], num_columns: usize) -> Vec<bool> {
            detect_json_columns(rows.len(), num_columns, |row, col| {
                rows[row].get(col).map(|cell| Cow::Borrowed(cell.as_str()))
            })
        }

        #[test]
        fn flags_all_json_column() {
            let rows = vec![
                vec![r#"{"id":1}"#.to_string()],
                vec![r#"{"id":2}"#.to_string()],
            ];

            assert_eq!(detect(&rows, 1), vec![true]);
        }

        #[test]
        fn does_not_flag_text_column() {
            let rows = vec![vec!["alice".to_string()], vec!["bob".to_string()]];

            assert_eq!(detect(&rows, 1), vec![false]);
        }

        #[test]
        fn skips_null_cells_so_partial_json_column_still_formats() {
            let rows = vec![
                vec![r#"{"a":1}"#.to_string()],
                vec![String::new()],
                vec![r#"{"a":2}"#.to_string()],
            ];

            assert_eq!(detect(&rows, 1), vec![true]);
        }

        #[test]
        fn mixed_column_is_not_json() {
            // One scalar disqualifies the column.
            let rows = vec![vec![r#"{"a":1}"#.to_string()], vec!["hello".to_string()]];

            assert_eq!(detect(&rows, 1), vec![false]);
        }

        #[test]
        fn all_null_column_is_not_json() {
            let rows = vec![vec![String::new()], vec![String::new()]];

            assert_eq!(detect(&rows, 1), vec![false]);
        }

        #[test]
        fn handles_multiple_columns_independently() {
            let rows = vec![
                vec!["1".to_string(), r#"{"a":1}"#.to_string(), "x".to_string()],
                vec!["2".to_string(), r#"{"b":2}"#.to_string(), "y".to_string()],
            ];

            assert_eq!(detect(&rows, 3), vec![false, true, false]);
        }

        #[test]
        fn no_rows_returns_all_false() {
            assert_eq!(detect(&[], 3), vec![false, false, false]);
        }

        #[test]
        fn samples_only_first_fifty_rows() {
            // 60 rows all JSON -> still detected; just confirming no panic.
            let rows: Vec<Vec<String>> = (0..60).map(|i| vec![format!(r#"{{"i":{i}}}"#)]).collect();

            assert_eq!(detect(&rows, 1), vec![true]);
        }
    }

    mod pretty_format_json {
        use super::*;

        #[test]
        fn formats_object_like_jsonb_pretty() {
            let formatted = pretty_format_json(r#"{"f1":1,"f2":[1,2,3]}"#, 0);

            assert_eq!(
                formatted,
                "{\n    \"f1\": 1,\n    \"f2\": [\n        1,\n        2,\n        3\n    ]\n}"
            );
        }

        #[test]
        fn formats_arrays() {
            let formatted = pretty_format_json(r#"[1,"two",{"k":"v"}]"#, 0);

            assert_eq!(
                formatted,
                "[\n    1,\n    \"two\",\n    {\n        \"k\": \"v\"\n    }\n]"
            );
        }

        #[test]
        fn empty_containers_render_compact() {
            assert_eq!(pretty_format_json("{}", 0), "{}");
            assert_eq!(pretty_format_json("[]", 0), "[]");
        }

        #[test]
        fn nested_indentation_grows_four_spaces_per_level() {
            let formatted = pretty_format_json(r#"{"a":{"b":{"c":1}}}"#, 0);

            assert_eq!(
                formatted,
                "{\n    \"a\": {\n        \"b\": {\n            \"c\": 1\n        }\n    }\n}"
            );
        }

        #[test]
        fn escapes_keys_like_serde_json() {
            let formatted = pretty_format_json(r#"{"a\"b":1}"#, 0);

            assert_eq!(formatted, "{\n    \"a\\\"b\": 1\n}");
        }

        #[test]
        fn invalid_json_returns_input_unchanged() {
            assert_eq!(pretty_format_json("not json", 0), "not json");
            assert_eq!(pretty_format_json("", 0), "");
        }

        #[test]
        fn trims_surrounding_whitespace_before_parsing() {
            let formatted = pretty_format_json("\n  {\"a\":1}  \n", 0);

            assert_eq!(formatted, "{\n    \"a\": 1\n}");
        }

        #[test]
        fn stays_compact_when_fits_within_max_width() {
            let json = r#"{"a":1,"b":2}"#;
            assert_eq!(pretty_format_json(json, json.len()), r#"{"a":1,"b":2}"#);
        }

        #[test]
        fn expands_when_compact_exceeds_max_width() {
            let formatted = pretty_format_json(r#"{"a":1}"#, 5);
            assert_eq!(formatted, "{\n    \"a\": 1\n}");
        }

        #[rstest]
        #[case("42", "42")]
        #[case(r#""hi""#, "\"hi\"")]
        #[case("true", "true")]
        #[case("null", "null")]
        fn scalars_render_compact(#[case] input: &str, #[case] expected: &str) {
            assert_eq!(pretty_format_json(input, 0), expected);
        }
    }

    mod format_cell {
        use super::*;

        #[test]
        fn borrows_input_when_formatting_disabled() {
            let out = format_cell(r#"{"a":1}"#, true, false);

            assert!(matches!(out, Cow::Borrowed(_)));
            assert_eq!(out.as_ref(), r#"{"a":1}"#);
        }

        #[test]
        fn borrows_input_when_column_is_not_json() {
            let out = format_cell("alice", false, true);

            assert!(matches!(out, Cow::Borrowed(_)));
            assert_eq!(out.as_ref(), "alice");
        }

        #[test]
        fn owns_formatted_output_when_enabled() {
            let out = format_cell(r#"{"a":1}"#, true, true);

            assert!(matches!(out, Cow::Owned(_)));
            assert_eq!(out.as_ref(), "{\n    \"a\": 1\n}");
        }
    }
}
