use super::auth::{authenticate, header_text};
use super::error::{ApiResult, api_error};
use super::image_contract::{detected_image_type, is_supported_image_media_type};
use crate::state::AppState;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;
const MAX: i64 = 25 * 1024 * 1024;
const CHUNK: usize = 1024 * 1024;
type UploadSessionRow = (
    String,
    String,
    String,
    String,
    i64,
    String,
    i64,
    String,
    Option<String>,
);
fn err(s: StatusCode, e: &str) -> (StatusCode, Json<Value>) {
    api_error(s, e)
}
pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let owner = state.primary_owner_id.clone();
    let device_id = authenticate(&state, &headers)?;
    state
        .store
        .ensure_owner_device(&owner, &device_id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let filename = urlencoding::decode(
        header_text(&headers, "x-voiceos-file-name")
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "file_name_required"))?,
    )
    .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_file_name"))?
    .into_owned();
    let media = header_text(&headers, "content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    let size: i64 = header_text(&headers, "x-voiceos-upload-length")
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "upload_length_required"))?
        .parse()
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_upload_length"))?;
    let hash = header_text(&headers, "x-voiceos-upload-sha256")
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "upload_sha256_required"))?
        .to_owned();
    if !is_supported_image_media_type(&media) {
        return Err(err(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ));
    }
    if size < 1 {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_upload_length"));
    }
    if size > MAX {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "upload_too_large"));
    }
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_upload_sha256"));
    }
    let id = Uuid::new_v4().to_string();
    let c = state
        .store
        .connection()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    c.execute("INSERT INTO upload_sessions(upload_id,owner_id,device_id,filename,media_type,byte_size,sha256,created_at) VALUES(?,?,?,?,?,?,?,?)",params![id,owner,device_id,filename,media,size,hash,Utc::now().to_rfc3339()]).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,&e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"upload":{"upload_id":id,"filename":filename,"media_type":media,"byte_size":size,"sha256":hash,"chunk_size":CHUNK,"received_bytes":0,"status":"created"}}),
        ),
    ))
}

pub(crate) async fn chunk(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, offset)): Path<(String, i64)>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    if offset < 0 || body.is_empty() || body.len() > CHUNK {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_chunk"));
    }
    let c = state
        .store
        .connection()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let row: Option<(String, String, i64, i64, String)> = c
        .query_row(
            "SELECT owner_id,device_id,byte_size,received_bytes,status FROM upload_sessions WHERE upload_id=?",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let Some((owner, upload_device_id, total, received, status)) = row else {
        return Err(err(StatusCode::NOT_FOUND, "upload_not_found"));
    };
    if owner != state.primary_owner_id || upload_device_id != device_id {
        return Err(err(StatusCode::NOT_FOUND, "upload_not_found"));
    }
    if status == "finalized" || offset > received || offset + body.len() as i64 > total {
        return Err(err(StatusCode::CONFLICT, "offset_conflict"));
    }
    if offset < received {
        let stored: Option<Vec<u8>> = c
            .query_row(
                "SELECT bytes FROM upload_chunks WHERE upload_id=? AND offset=?",
                params![id, offset],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        if stored.as_deref() != Some(body.as_ref()) {
            return Err(err(StatusCode::CONFLICT, "offset_conflict"));
        }
        return Ok(Json(
            json!({"upload_id":id,"received_bytes":received,"next_offset":received,"status":"uploading"}),
        ));
    }
    c.execute(
        "INSERT INTO upload_chunks(upload_id,offset,bytes) VALUES(?,?,?)",
        params![id, offset, body.as_ref()],
    )
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let next = received + body.len() as i64;
    c.execute(
        "UPDATE upload_sessions SET received_bytes=?,status='uploading' WHERE upload_id=?",
        params![next, id],
    )
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(
        json!({"upload_id":id,"received_bytes":next,"next_offset":next,"status":"uploading"}),
    ))
}

pub(crate) async fn finalize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authenticate(&state, &headers)?;
    let c = state
        .store
        .connection()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let s: Option<UploadSessionRow> = c
        .query_row(
            "SELECT owner_id,device_id,filename,media_type,byte_size,sha256,received_bytes,status,attachment_id FROM upload_sessions WHERE upload_id=?",
            [&id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .optional()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let Some((owner, device_id, file, media, total, want, got, status, attachment_id)) = s else {
        return Err(err(StatusCode::NOT_FOUND, "upload_not_found"));
    };
    if owner != state.primary_owner_id {
        return Err(err(StatusCode::NOT_FOUND, "upload_not_found"));
    };
    if status == "finalized" {
        let attachment_id = attachment_id.ok_or_else(|| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "finalized_upload_missing_attachment",
            )
        })?;
        let a:Value=c.query_row("SELECT json_object('attachment_id',attachment_id,'filename',filename,'media_type',media_type,'byte_size',byte_size,'sha256',sha256,'status','ready') FROM attachments WHERE attachment_id=?",[attachment_id],|r|r.get::<_,String>(0)).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,&e.to_string()))?.parse().unwrap();
        return Ok((StatusCode::OK, Json(json!({"attachment":a}))));
    }
    if got != total {
        return Err(err(StatusCode::CONFLICT, "upload_incomplete"));
    };
    let mut st = c
        .prepare("SELECT bytes FROM upload_chunks WHERE upload_id=? ORDER BY offset")
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let parts = st
        .query_map([&id], |r| r.get::<_, Vec<u8>>(0))
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let mut data = Vec::new();
    for p in parts {
        data.extend(p.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?)
    }
    let actual = format!("{:x}", Sha256::digest(&data));
    if actual != want {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "sha256_mismatch"));
    };
    if detected_image_type(&data) != Some(media.as_str()) {
        return Err(err(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "attachment_media_type_mismatch",
        ));
    }
    let aid = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    c.execute(
        "INSERT INTO attachments(attachment_id,owner_id,device_id,filename,media_type,byte_size,sha256,source_bytes,status,created_at,expires_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
        params![
            aid,
            owner,
            device_id,
            file,
            media,
            total,
            want,
            data,
            "uploaded",
            created_at,
            (Utc::now() + chrono::Duration::days(7)).to_rfc3339(),
        ],
    )
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    c.execute(
        "UPDATE upload_sessions SET status='finalized', attachment_id=? WHERE upload_id=?",
        params![aid, id],
    )
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"attachment":{"attachment_id":aid,"filename":file,"media_type":media,"byte_size":total,"sha256":want,"status":"ready"}}),
        ),
    ))
}
