use riphttplib::utils::parse_header;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_regular_header() {
        let header = parse_header("Content-Type: application/json").unwrap();
        assert_eq!(header.name, "Content-Type");
        assert_eq!(header.value, Some("application/json".to_string()));
    }

    #[test]
    fn test_parse_header_with_escape_sequences() {
        let header = parse_header("X-Test: line1\\nline2\\tindented").unwrap();
        assert_eq!(header.name, "X-Test");
        assert_eq!(header.value, Some("line1\nline2\tindented".to_string()));
    }

    #[test]
    fn test_parse_pseudo_header() {
        let header = parse_header(":method: GET").unwrap();
        assert_eq!(header.name, ":method");
        assert_eq!(header.value, Some("GET".to_string()));
    }

    #[test]
    fn test_parse_valueless_header() {
        let header = parse_header("Authorization").unwrap();
        assert_eq!(header.name, "Authorization");
        assert_eq!(header.value, None);
    }

    #[test]
    fn test_parse_pseudo_header_without_value() {
        let header = parse_header(":authority").unwrap();
        assert_eq!(header.name, ":authority");
        assert_eq!(header.value, None);
    }

    #[test]
    fn test_parse_header_with_multiple_colons() {
        let header = parse_header("X-Time: 12:34:56").unwrap();
        assert_eq!(header.name, "X-Time");
        assert_eq!(header.value, Some("12:34:56".to_string()));
    }

    #[test]
    fn test_parse_header_with_backslash_escapes() {
        let header = parse_header("X-Path: C:\\\\\\\\Users\\\\\\\\test").unwrap();
        assert_eq!(header.name, "X-Path");
        assert_eq!(header.value, Some("C:\\\\Users\\\test".to_string()));
    }
}
