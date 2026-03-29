use std::borrow::Cow;
use std::time::Duration;

use reqwest::Method;
use reqwest_middleware::{ClientBuilder as MiddlewareClientBuilder, ClientWithMiddleware};
use reqwest_tracing::{ReqwestOtelSpanBackend, TracingMiddleware};
use thiserror::Error;
use tokio_tungstenite::tungstenite::http::Extensions;
use tracing::{instrument, Span};

use crate::discord::Id;
use crate::model::mesastream::{
    CreatePlayerRequest, CurrentTrackResponse, EnqueueRequest, PlayerStateSnapshot,
    PlaylistSnapshot, SeekRequest, StatusResponse, Track, UpdateConnectionRequest, VolumeRequest,
};

const REQUEST_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Error)]
pub enum MesastreamError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Middleware error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("API error ({status}): {code} – {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },

    #[error("{0}")]
    Other(String),
}

pub type MesastreamResult<T> = Result<T, MesastreamError>;

pub struct MesastreamClient {
    client: ClientWithMiddleware,
    base_url: Cow<'static, str>,
    token: Cow<'static, str>,
}

// Custom span backend that treats 4xx responses as warnings instead of errors
#[derive(Clone, Debug)]
struct MesastreamSpanBackend;

impl ReqwestOtelSpanBackend for MesastreamSpanBackend {
    fn on_request_start(req: &reqwest::Request, _ext: &mut Extensions) -> Span {
        let method = req.method().as_str();
        let url = req.url().as_str();
        tracing::info_span!(
            "mesastream_client",
            http.request.method = %method,
            url.full = %url,
            http.response.status_code = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
        )
    }

    fn on_request_end(
        span: &Span,
        outcome: &Result<reqwest::Response, reqwest_middleware::Error>,
        _ext: &mut Extensions,
    ) {
        match outcome {
            Ok(response) => {
                let status = response.status();
                span.record("http.response.status_code", status.as_u16());

                if status.is_server_error() {
                    span.record("otel.status_code", "ERROR");
                } else {
                    span.record("otel.status_code", "OK");
                }
            }
            Err(e) => {
                span.record("otel.status_code", "ERROR");
                tracing::error!(parent: span, error = %e, "Request failed");
            }
        }
    }
}

impl MesastreamClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        let reqwest_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .user_agent(format!(
                "bm-lib/{} mesastream-client",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .unwrap_or_else(|error| {
                tracing::error!(error = %error, "Failed to build mesastream HTTP client, using fallback");
                reqwest::Client::new()
            });

        let client = MiddlewareClientBuilder::new(reqwest_client)
            .with(TracingMiddleware::<MesastreamSpanBackend>::new())
            .build();

        Self {
            client,
            base_url: Cow::Owned(base_url.into().trim_end_matches('/').to_owned()),
            token: Cow::Owned(token.into()),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn send<R>(&self, method: Method, path: &str) -> MesastreamResult<R>
    where
        R: serde::de::DeserializeOwned,
    {
        self.send_with_body::<(), R>(method, path, None).await
    }

    async fn send_with_body<T, R>(
        &self,
        method: Method,
        path: &str,
        body: Option<&T>,
    ) -> MesastreamResult<R>
    where
        T: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let url = self.url(path);
        let mut req = self
            .client
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", self.token));

        if let Some(body) = body {
            let bytes = serde_json::to_vec(body).map_err(MesastreamError::Json)?;
            req = req.header("Content-Type", "application/json").body(bytes);
        }

        let response = req.send().await.map_err(MesastreamError::Middleware)?;
        let status = response.status();

        if !status.is_success() {
            let code = status.as_u16();
            // Try to deserialise the error body; fall back to a plain string.
            let bytes = response.bytes().await.unwrap_or_default();

            if let Ok(err) =
                serde_json::from_slice::<crate::model::mesastream::ErrorResponse>(&bytes)
            {
                return Err(MesastreamError::Api {
                    status: code,
                    code: err.code,
                    message: err.message,
                });
            }

            let body_text = String::from_utf8_lossy(&bytes).into_owned();
            return Err(MesastreamError::Other(format!("HTTP {code}: {body_text}")));
        }

        let payload = response.json::<R>().await.map_err(MesastreamError::Http)?;
        Ok(payload)
    }

    #[instrument(skip(self))]
    pub async fn health(&self) -> MesastreamResult<StatusResponse> {
        self.send(Method::GET, "/health").await
    }

    #[instrument(skip(self))]
    pub async fn ready(&self) -> MesastreamResult<StatusResponse> {
        self.send(Method::GET, "/ready").await
    }

    #[instrument(skip(self, payload))]
    pub async fn create_player(
        &self,
        payload: &CreatePlayerRequest,
    ) -> MesastreamResult<PlayerStateSnapshot> {
        self.send_with_body(Method::POST, "/players", Some(payload))
            .await
    }

    #[instrument(skip(self))]
    pub async fn list_players(&self) -> MesastreamResult<Vec<PlayerStateSnapshot>> {
        self.send(Method::GET, "/players").await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn get_player(&self, id: &Id) -> MesastreamResult<PlayerStateSnapshot> {
        self.send(Method::GET, &format!("/players/{id}")).await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn delete_player(&self, id: &Id) -> MesastreamResult<()> {
        let url = self.url(&format!("/players/{id}"));
        let response = self
            .client
            .request(Method::DELETE, &url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(MesastreamError::Middleware)?;

        let status = response.status();
        if !status.is_success() {
            let code = status.as_u16();
            let bytes = response.bytes().await.unwrap_or_default();
            if let Ok(err) =
                serde_json::from_slice::<crate::model::mesastream::ErrorResponse>(&bytes)
            {
                return Err(MesastreamError::Api {
                    status: code,
                    code: err.code,
                    message: err.message,
                });
            }
            let body_text = String::from_utf8_lossy(&bytes).into_owned();
            return Err(MesastreamError::Other(format!("HTTP {code}: {body_text}")));
        }
        Ok(())
    }

    #[instrument(skip(self, payload), fields(player_id = %id))]
    pub async fn update_connection(
        &self,
        id: &Id,
        payload: &UpdateConnectionRequest,
    ) -> MesastreamResult<PlayerStateSnapshot> {
        self.send_with_body(
            Method::PUT,
            &format!("/players/{id}/connection"),
            Some(payload),
        )
        .await
    }

    #[instrument(skip(self), fields(player_id = %id, url = %url))]
    pub async fn enqueue(&self, id: &Id, url: &str) -> MesastreamResult<PlayerStateSnapshot> {
        let body = EnqueueRequest {
            url: url.to_owned(),
        };
        self.send_with_body(Method::POST, &format!("/players/{id}/queue"), Some(&body))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn get_queue(&self, id: &Id) -> MesastreamResult<Vec<Track>> {
        self.send(Method::GET, &format!("/players/{id}/queue"))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn clear_queue(&self, id: &Id) -> MesastreamResult<PlayerStateSnapshot> {
        self.send(Method::DELETE, &format!("/players/{id}/queue"))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id, name = %name))]
    pub async fn save_playlist(&self, id: &Id, name: &str) -> MesastreamResult<PlaylistSnapshot> {
        self.send_with_body::<(), _>(
            Method::POST,
            &format!("/players/{id}/playlists/{name}"),
            None,
        )
        .await
    }

    #[instrument(skip(self), fields(player_id = %id, name = %name))]
    pub async fn enqueue_playlist(
        &self,
        id: &Id,
        name: &str,
    ) -> MesastreamResult<PlayerStateSnapshot> {
        self.send_with_body::<(), _>(
            Method::POST,
            &format!("/players/{id}/playlists/{name}/enqueue"),
            None,
        )
        .await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn play(&self, id: &Id) -> MesastreamResult<PlayerStateSnapshot> {
        self.send(Method::POST, &format!("/players/{id}/play"))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn pause(&self, id: &Id) -> MesastreamResult<PlayerStateSnapshot> {
        self.send(Method::POST, &format!("/players/{id}/pause"))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn resume(&self, id: &Id) -> MesastreamResult<PlayerStateSnapshot> {
        self.send(Method::POST, &format!("/players/{id}/resume"))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn skip(&self, id: &Id) -> MesastreamResult<PlayerStateSnapshot> {
        self.send(Method::POST, &format!("/players/{id}/skip"))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn stop(&self, id: &Id) -> MesastreamResult<PlayerStateSnapshot> {
        self.send(Method::POST, &format!("/players/{id}/stop"))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id, position_ms))]
    pub async fn seek(&self, id: &Id, position_ms: u64) -> MesastreamResult<PlayerStateSnapshot> {
        let body = SeekRequest { position_ms };
        self.send_with_body(Method::POST, &format!("/players/{id}/seek"), Some(&body))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id, volume))]
    pub async fn set_volume(&self, id: &Id, volume: f32) -> MesastreamResult<PlayerStateSnapshot> {
        let body = VolumeRequest { volume };
        self.send_with_body(Method::POST, &format!("/players/{id}/volume"), Some(&body))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn get_current_track(&self, id: &Id) -> MesastreamResult<Option<CurrentTrackResponse>> {
        self.send(Method::GET, &format!("/players/{id}/current"))
            .await
    }

    #[instrument(skip(self), fields(player_id = %id))]
    pub async fn get_status(&self, id: &Id) -> MesastreamResult<PlayerStateSnapshot> {
        self.send(Method::GET, &format!("/players/{id}/status"))
            .await
    }
}
