pub(crate) fn is_supported_image_media_type(media_type: &str) -> bool {
    matches!(media_type, "image/jpeg" | "image/png" | "image/webp")
}

pub(crate) fn detected_image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{detected_image_type, is_supported_image_media_type};

    #[test]
    fn accepts_only_contract_image_media_types() {
        assert!(is_supported_image_media_type("image/png"));
        assert!(is_supported_image_media_type("image/jpeg"));
        assert!(is_supported_image_media_type("image/webp"));
        assert!(!is_supported_image_media_type("image/gif"));
        assert!(!is_supported_image_media_type("application/pdf"));
        assert!(!is_supported_image_media_type("image/svg+xml"));
    }

    #[test]
    fn recognizes_only_contract_image_signatures() {
        assert_eq!(
            detected_image_type(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(
            detected_image_type(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(
            detected_image_type(b"RIFF\x10\0\0\0WEBPVP8 rest"),
            Some("image/webp")
        );
        assert_eq!(detected_image_type(b"not an image"), None);
    }
}
