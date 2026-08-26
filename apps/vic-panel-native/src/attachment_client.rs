use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const DEVICE_ID: &str = "vic-native-panel";
const MAX_BYTES: usize = 25 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct Attachment {
    #[serde(rename = "attachment_id")]
    pub id: String,
    pub filename: String,
    pub media_type: String,
}

#[derive(Debug, Deserialize)]
struct UploadInfo {
    upload_id: String,
    chunk_size: usize,
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    upload: UploadInfo,
}

#[derive(Debug, Deserialize)]
struct FinalizeResponse {
    attachment: Attachment,
}

pub fn media_type_for_path(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

pub fn upload(gateway: &str, path: &Path) -> Result<Attachment, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err("Image must be between 1 byte and 25 MiB".into());
    }
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid filename")?;
    let media_type = media_type_for_path(path).ok_or("Choose a JPEG, PNG, or WebP image")?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let mut response = ureq::post(format!("{gateway}/v1/uploads"))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .header("X-VoiceOS-File-Name", filename)
        .header("X-VoiceOS-Upload-Length", bytes.len().to_string())
        .header("X-VoiceOS-Upload-SHA256", &hash)
        .content_type(media_type)
        .send_empty()
        .map_err(|error| error.to_string())?;
    let created: UploadResponse = response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    let mut offset = 0;
    while offset < bytes.len() {
        let end = (offset + created.upload.chunk_size).min(bytes.len());
        ureq::put(format!(
            "{gateway}/v1/uploads/{}/chunks/{offset}",
            created.upload.upload_id
        ))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .content_type("application/octet-stream")
        .send(&bytes[offset..end])
        .map_err(|error| error.to_string())?;
        offset = end;
    }
    let mut response = ureq::post(format!(
        "{gateway}/v1/uploads/{}/finalize",
        created.upload.upload_id
    ))
    .header("X-VoiceOS-Device-ID", DEVICE_ID)
    .send_empty()
    .map_err(|error| error.to_string())?;
    response
        .body_mut()
        .read_json::<FinalizeResponse>()
        .map_err(|error| error.to_string())
        .map(|value| value.attachment)
}

#[cfg(test)]
mod tests {
    use super::{FinalizeResponse, UploadResponse, media_type_for_path};

    #[test]
    fn accepts_contract_image_types_and_rejects_other_extensions() {
        assert_eq!(
            media_type_for_path(std::path::Path::new("photo.JPG")),
            Some("image/jpeg")
        );
        assert_eq!(
            media_type_for_path(std::path::Path::new("photo.png")),
            Some("image/png")
        );
        assert_eq!(
            media_type_for_path(std::path::Path::new("photo.webp")),
            Some("image/webp")
        );
        assert_eq!(media_type_for_path(std::path::Path::new("photo.gif")), None);
        assert_eq!(media_type_for_path(std::path::Path::new("photo.txt")), None);
    }

    #[test]
    fn parses_nested_upload_response() {
        let created: UploadResponse =
            serde_json::from_str(r#"{"upload":{"upload_id":"u1","chunk_size":3}}"#).unwrap();
        assert_eq!(created.upload.upload_id, "u1");
        assert_eq!(created.upload.chunk_size, 3);
    }

    #[test]
    fn parses_contract_attachment_id() {
        let finalized: FinalizeResponse = serde_json::from_str(
            r#"{"attachment":{"attachment_id":"a1","filename":"photo.png","media_type":"image/png"}}"#,
        )
        .unwrap();
        assert_eq!(finalized.attachment.id, "a1");
    }
}
