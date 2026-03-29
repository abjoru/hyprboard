/// Simple percent-decoding for file:// URLs
pub fn urlencoding_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let hi = iter.next().and_then(hex_val);
            let lo = iter.next().and_then(hex_val);
            if let (Some(h), Some(l)) = (hi, lo) {
                bytes.push(h << 4 | l);
            }
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_no_encoding() {
        assert_eq!(
            urlencoding_decode("/home/user/file.png"),
            "/home/user/file.png"
        );
    }

    #[test]
    fn decode_spaces() {
        assert_eq!(
            urlencoding_decode("/home/user/my%20file.png"),
            "/home/user/my file.png"
        );
    }

    #[test]
    fn decode_special_chars() {
        assert_eq!(urlencoding_decode("hello%21%40%23"), "hello!@#");
    }

    #[test]
    fn decode_uppercase_hex() {
        assert_eq!(urlencoding_decode("%2F%2E"), "/.");
    }

    #[test]
    fn decode_lowercase_hex() {
        assert_eq!(urlencoding_decode("%2f%2e"), "/.");
    }

    #[test]
    fn decode_mixed() {
        assert_eq!(
            urlencoding_decode("/path/to/My%20Photos%20%282024%29/img.png"),
            "/path/to/My Photos (2024)/img.png"
        );
    }

    #[test]
    fn decode_empty() {
        assert_eq!(urlencoding_decode(""), "");
    }

    #[test]
    fn decode_trailing_percent() {
        assert_eq!(urlencoding_decode("abc%"), "abc");
    }

    #[test]
    fn decode_partial_percent() {
        assert_eq!(urlencoding_decode("abc%2"), "abc");
    }

    #[test]
    fn decode_utf8_multibyte() {
        // é = U+00E9 = %C3%A9 in UTF-8
        assert_eq!(urlencoding_decode("caf%C3%A9"), "café");
        // ü = U+00FC = %C3%BC
        assert_eq!(urlencoding_decode("Gn%C3%BCbel"), "Gnübel");
    }

    #[test]
    fn hex_val_digits() {
        assert_eq!(hex_val(b'0'), Some(0));
        assert_eq!(hex_val(b'9'), Some(9));
    }

    #[test]
    fn hex_val_lowercase() {
        assert_eq!(hex_val(b'a'), Some(10));
        assert_eq!(hex_val(b'f'), Some(15));
    }

    #[test]
    fn hex_val_uppercase() {
        assert_eq!(hex_val(b'A'), Some(10));
        assert_eq!(hex_val(b'F'), Some(15));
    }

    #[test]
    fn hex_val_invalid() {
        assert_eq!(hex_val(b'g'), None);
        assert_eq!(hex_val(b'Z'), None);
        assert_eq!(hex_val(b' '), None);
    }
}
