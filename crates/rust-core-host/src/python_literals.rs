//! Conservative runtime values for single-line Python literal equivalence proofs.
//!
//! Prefixes are syntax: `b` changes the value's type, while `r` changes escape
//! interpretation. Unsupported forms decline proof rather than compare source bodies.

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LiteralValue {
    pub(crate) is_bytes: bool,
    pub(crate) value: String,
}

pub(crate) fn decode(raw: &str) -> Option<LiteralValue> {
    let raw = raw.trim();
    let quote_start = raw.find(['\'', '"'])?;
    let prefix = raw[..quote_start].to_ascii_lowercase();
    if !matches!(prefix.as_str(), "" | "u" | "r" | "b" | "br" | "rb") {
        return None;
    }
    let literal = &raw[quote_start..];
    let quote = literal.chars().next()?;
    if literal.len() < 2
        || !literal.ends_with(quote)
        || literal.starts_with("'''")
        || literal.starts_with("\"\"\"")
    {
        return None;
    }
    let inner = &literal[1..literal.len() - 1];
    let is_bytes = prefix.contains('b');
    if inner.contains(['\n', '\r']) || (is_bytes && !inner.is_ascii()) {
        return None;
    }
    let is_raw = prefix.contains('r');
    let mut chars = inner.chars().peekable();
    let mut value = String::new();
    while let Some(ch) = chars.next() {
        if ch == quote {
            return None; // An unescaped delimiter or concatenated literal.
        }
        if ch != '\\' {
            value.push(ch);
            continue;
        }
        let escaped = chars.next()?;
        if is_raw {
            value.push('\\');
            value.push(escaped);
            continue;
        }
        match escaped {
            '\\' | '\'' | '"' => value.push(escaped),
            'a' => value.push('\x07'),
            'b' => value.push('\x08'),
            'f' => value.push('\x0c'),
            'n' => value.push('\n'),
            'r' => value.push('\r'),
            't' => value.push('\t'),
            'v' => value.push('\x0b'),
            '0'..='7' => {
                let mut code = escaped.to_digit(8)?;
                for _ in 0..2 {
                    let Some(digit) = chars.peek().and_then(|c| c.to_digit(8)) else {
                        break;
                    };
                    chars.next();
                    code = code * 8 + digit;
                }
                if is_bytes && code > 255 {
                    return None;
                }
                value.push(char::from_u32(code)?);
            }
            'x' | 'u' | 'U' => {
                if is_bytes && escaped != 'x' {
                    return None;
                }
                let count = match escaped {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let mut code = 0u32;
                for _ in 0..count {
                    code = code
                        .checked_mul(16)?
                        .checked_add(chars.next()?.to_digit(16)?)?;
                }
                value.push(char::from_u32(code)?);
            }
            _ => return None, // Includes named Unicode and unrecognized escapes.
        }
    }
    Some(LiteralValue { is_bytes, value })
}

pub(crate) fn equivalent_values(old: &str, new: &str) -> Option<(String, String)> {
    let old = decode(old)?;
    let new = decode(new)?;
    (old == new).then_some((old.value, new.value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_python_literals_decline_equivalence_proof() {
        for raw in [
            "f'{value}'",
            "'''plain'''",
            "'first' 'second'",
            "r'last\\'",
            "b'雪'",
            "'\\N{SNOWMAN}'",
            "'\\d'",
            "'\\ud800'",
            "b'\\400'",
            "'\\Uffffffff'",
            "ur'plain'",
            "'unclosed",
            "'two\nlines'",
        ] {
            assert!(
                decode(raw).is_none(),
                "unsupported literal accepted: {raw:?}"
            );
        }
    }

    #[test]
    fn octal_escapes_consume_at_most_three_digits() {
        assert_eq!(decode(r"'\0001'").unwrap().value, "\01");
        assert_ne!(decode(r"'\0001'"), decode(r"'\001'"));
        assert_eq!(decode(r"'\101'"), decode("'A'"));
    }

    #[test]
    fn raw_literals_preserve_backslashes_and_escaped_delimiters() {
        assert_eq!(decode(r#"r"\'\n""#).unwrap().value, "\\'\\n");
        assert_ne!(decode(r#"r"\'\n""#), decode(r#""\'\n""#));
    }
}
