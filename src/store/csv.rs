//! Minimal RFC 4180 CSV writer and reader.
//!
//! Only what the export needs: quote a field when it contains a comma,
//! double quote, CR or LF; double the quotes inside quoted fields; CRLF
//! line endings. The reader exists so tests can round-trip exported files
//! without pulling in an external CSV crate.

use std::io::Write;

/// Writes one CSV record (fields + CRLF).
pub fn write_row<W: Write>(w: &mut W, fields: &[String]) -> std::io::Result<()> {
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            w.write_all(b",")?;
        }
        w.write_all(escape(field).as_bytes())?;
    }
    w.write_all(b"\r\n")
}

fn escape(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Parses one CSV record into fields. Handles quoted fields with embedded
/// commas, escaped double quotes and CRLF/LF endings.
pub fn split_row(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => {
                    out.push(std::mem::take(&mut field));
                }
                '\r' => {}
                '\n' => break,
                _ => field.push(c),
            }
        }
    }
    out.push(field);
    out
}

/// Parses a whole CSV document into records. Handles quoted fields with
/// embedded commas, escaped double quotes and embedded CRLF/LF (RFC 4180
/// allows multi-line fields).
pub fn records(content: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => record.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    record.push(std::mem::take(&mut field));
                    out.push(std::mem::take(&mut record));
                }
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        out.push(record);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_only_when_needed() {
        assert_eq!(escape("plain"), "plain");
        assert_eq!(escape("a,b"), "\"a,b\"");
        assert_eq!(escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(escape("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn round_trip_tricky_fields() {
        let fields = vec![
            "simple".to_string(),
            "with,comma".to_string(),
            "with\"quote".to_string(),
            "with\ntwo\nlines".to_string(),
            "".to_string(),
            "приор из литературы, «резонанс»".to_string(),
        ];
        let mut buf: Vec<u8> = Vec::new();
        write_row(&mut buf, &fields).unwrap();
        let line = String::from_utf8(buf).unwrap();
        assert!(line.ends_with("\r\n"));
        assert_eq!(split_row(&line), fields);
    }

    #[test]
    fn records_multiline_document() {
        let doc = "a,\"b\rc\"\r\n1,2\r\n\"x\"\"y\",z\n";
        let recs = records(doc);
        assert_eq!(
            recs,
            vec![
                vec!["a".to_string(), "b\rc".to_string()],
                vec!["1".to_string(), "2".to_string()],
                vec!["x\"y".to_string(), "z".to_string()],
            ]
        );
    }
}
