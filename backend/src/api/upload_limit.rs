//! MCAP file size policy, configured once when composing the router.
use anyhow::{ensure, Context, Result};

#[derive(Clone, Copy)]
pub(super) struct UploadLimit(Option<u64>);

impl UploadLimit {
    pub(super) fn from_env() -> Result<Self> {
        Self::parse(std::env::var("MCAP_UPLOAD_LIMIT_GIB").ok().as_deref())
    }

    fn parse(value: Option<&str>) -> Result<Self> {
        let gib = value.unwrap_or("64").trim().parse::<u64>().context(
            "MCAP_UPLOAD_LIMIT_GIB must be a nonnegative whole number (0 means unlimited)",
        )?;
        let bytes = gib
            .checked_mul(1024 * 1024 * 1024)
            .context("MCAP_UPLOAD_LIMIT_GIB is too large")?;
        Ok(Self((bytes != 0).then_some(bytes)))
    }

    pub(super) fn max_bytes(self) -> Option<u64> {
        self.0
    }

    pub(super) fn add(self, received: u64, additional: usize) -> Result<u64> {
        let total = received
            .checked_add(u64::try_from(additional)?)
            .context("MCAP upload size overflow")?;
        if let Some(limit) = self.0 {
            let size = if limit >= 1024 * 1024 * 1024 {
                format!("{} GiB", limit / (1024 * 1024 * 1024))
            } else {
                format!("{limit} bytes")
            };
            ensure!(
                total <= limit,
                "MCAP upload exceeds the configured limit of {size}. The file is too large. Choose a smaller MCAP file."
            );
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_unlimited_and_invalid_limits() {
        assert_eq!(
            UploadLimit::parse(None).unwrap().0,
            Some(64 * 1024 * 1024 * 1024)
        );
        assert_eq!(
            UploadLimit::parse(Some(" 2 ")).unwrap().0,
            Some(2 * 1024 * 1024 * 1024)
        );
        assert_eq!(UploadLimit::parse(Some("0")).unwrap().0, None);
        for value in ["", "-1", "1.5", "unlimited", "18446744073709551615"] {
            assert!(UploadLimit::parse(Some(value)).is_err(), "accepted {value}");
        }
    }

    #[tokio::test]
    async fn rejects_oversized_uploads_without_publishing_or_leaving_staged_files() {
        use axum::{
            body::Body,
            extract::DefaultBodyLimit,
            http::{Request, StatusCode},
            routing::post,
            Extension, Router,
        };
        use http_body_util::BodyExt;
        use serde_json::{json, Value};
        use std::{fs, sync::Arc};
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(crate::Store::new(dir.path()).unwrap());
        let project = store
            .create_project(&json!({"name":"Upload limit"}))
            .unwrap();
        let id = project["uuid"].as_str().unwrap();
        let payload = mcap::MAGIC.repeat(2);
        let mut body = b"--limit\r\nContent-Disposition: form-data; name=\"file\"; filename=\"source.mcap\"\r\n\r\n".to_vec();
        body.extend(&payload);
        body.extend(b"\r\n--limit--\r\n");
        // The limit applies to file bytes, excluding the multipart envelope.
        for (limit, expected) in [(15, StatusCode::PAYLOAD_TOO_LARGE), (16, StatusCode::OK)] {
            let app = Router::new()
                .route(
                    "/projects/{project}/recordings",
                    post(super::super::recordings::import_recording),
                )
                .layer(DefaultBodyLimit::disable())
                .layer(Extension(UploadLimit(Some(limit))))
                .with_state(store.clone());
            let response = app
                .oneshot(
                    Request::post(format!(
                        "/projects/{id}/recordings?defer_index=true&auto_translate=false"
                    ))
                    .header("content-type", "multipart/form-data; boundary=limit")
                    .body(Body::from(body.clone()))
                    .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            let value: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            if expected == StatusCode::PAYLOAD_TOO_LARGE {
                assert!(value["error"]
                    .as_str()
                    .unwrap()
                    .contains("file is too large"));
                assert!(store.project(id).unwrap()["mcaps"]
                    .as_array()
                    .unwrap()
                    .is_empty());
                let folder = std::path::Path::new(project["path"].as_str().unwrap()).join("mcap");
                assert_eq!(fs::read_dir(folder).unwrap().count(), 0);
            }
        }
    }
}
