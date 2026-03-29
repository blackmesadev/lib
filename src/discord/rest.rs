use crate::discord::{
    error::DiscordResult,
    model::{Channel, Gateway, Member, Role, User},
};
use dashmap::DashMap;
use reqwest::{header::HeaderMap, Client, Method, Response};
use reqwest_middleware::{ClientBuilder as MiddlewareClientBuilder, ClientWithMiddleware};
use reqwest_tracing::TracingMiddleware;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::instrument;

use super::{DiscordError, Embed, Guild, Id, Message};

const API_BASE: &str = "https://discord.com/api/v10";
const REQUEST_TIMEOUT_SECS: u64 = 10;
const USER_ERROR_DISCORD_REQUEST: &str = "An error occurred while communicating with Discord.";
const USER_ERROR_DISCORD_RESPONSE: &str = "An error occurred while processing a Discord response.";

#[derive(Debug, Clone)]
struct RateLimit {
    remaining: u32,
    reset_after: f64,
    is_global: bool,
}

impl RateLimit {
    fn from_headers(headers: &HeaderMap) -> Option<Self> {
        let remaining = headers
            .get("x-ratelimit-remaining")?
            .to_str()
            .ok()?
            .parse()
            .ok()?;
        let reset_after = headers
            .get("x-ratelimit-reset-after")?
            .to_str()
            .ok()?
            .parse()
            .ok()?;
        let is_global = headers.contains_key("x-ratelimit-global");

        Some(RateLimit {
            remaining,
            reset_after,
            is_global,
        })
    }

    fn should_wait(&self) -> Option<Duration> {
        if self.remaining == 0 {
            return Some(Duration::from_secs_f64(self.reset_after));
        }
        None
    }
}

pub struct DiscordRestClient {
    client: ClientWithMiddleware,
    token: Cow<'static, str>,
    rate_limits: DashMap<String, RateLimit>,
    global_rate_limit: RwLock<Option<(Instant, Duration)>>,
}

impl DiscordRestClient {
    pub fn new(token: &str) -> Self {
        let reqwest_client = Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .user_agent(format!(
                "DiscordBot (github.com/blackmesadev/lib, {})",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .unwrap_or_else(|error| {
                tracing::error!(error = %error, "Failed to construct configured HTTP client, using fallback client");
                Client::new()
            });

        let client = MiddlewareClientBuilder::new(reqwest_client)
            .with(TracingMiddleware::default())
            .build();

        tracing::info!("Discord REST client initialized");

        let trimmed_token = token.trim().to_string();

        Self {
            client,
            token: Cow::Owned(trimmed_token),
            rate_limits: DashMap::new(),
            global_rate_limit: RwLock::new(None),
        }
    }

    #[inline]
    fn build_url(&self, path: &str) -> String {
        format!("{}{}", API_BASE, path)
    }

    #[inline]
    fn channel_messages_path(channel_id: &Id) -> String {
        format!("/channels/{}/messages", channel_id)
    }

    #[inline]
    fn message_path(channel_id: &Id, message_id: &Id) -> String {
        format!("/channels/{}/messages/{}", channel_id, message_id)
    }

    #[inline]
    fn build_audit_log_headers(reason: Option<impl AsRef<str>>) -> Option<HeaderMap> {
        let reason = reason?;
        let Ok(header_value) = reason.as_ref().parse() else {
            tracing::warn!("Invalid X-Audit-Log-Reason header value, dropping reason");
            return None;
        };

        let mut headers = HeaderMap::new();
        headers.insert("X-Audit-Log-Reason", header_value);
        Some(headers)
    }

    #[instrument(skip(self, data, headers), fields(method = ?method, path = path, endpoint = endpoint))]
    async fn request_json<T, R>(
        &self,
        method: Method,
        path: &str,
        data: Option<T>,
        headers: Option<HeaderMap>,
        endpoint: &str,
    ) -> DiscordResult<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let response = match self
            .make_request(method, path, data, headers, endpoint)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(error = %error, endpoint = endpoint, "Discord request failed");
                return Err(DiscordError::Other(USER_ERROR_DISCORD_REQUEST.into()));
            }
        };

        match response.json().await {
            Ok(payload) => Ok(payload),
            Err(error) => {
                tracing::error!(error = %error, endpoint = endpoint, "Failed to parse Discord response body");
                Err(DiscordError::Other(USER_ERROR_DISCORD_RESPONSE.into()))
            }
        }
    }

    #[instrument(skip(self, data, headers), fields(method = ?method, path = path, endpoint = endpoint))]
    async fn request_empty<T>(
        &self,
        method: Method,
        path: &str,
        data: Option<T>,
        headers: Option<HeaderMap>,
        endpoint: &str,
    ) -> DiscordResult<()>
    where
        T: Serialize,
    {
        match self
            .make_request(method, path, data, headers, endpoint)
            .await
        {
            Ok(_) => Ok(()),
            Err(error) => {
                tracing::error!(error = %error, endpoint = endpoint, "Discord request failed");
                Err(DiscordError::Other(USER_ERROR_DISCORD_REQUEST.into()))
            }
        }
    }

    #[instrument(skip(self, data), fields(method = ?method, action = action))]
    fn spawn_fire_and_forget(
        &self,
        method: Method,
        path: String,
        data: Option<Value>,
        action: &'static str,
    ) {
        let client = self.client.clone();
        let token = self.token.clone();
        let url = self.build_url(&path);

        tokio::spawn(async move {
            let mut request = client
                .request(method, &url)
                .header("Authorization", token.as_ref());

            if let Some(data) = data {
                if let Ok(body) = serde_json::to_string(&data) {
                    request = request
                        .header("Content-Type", "application/json")
                        .body(body);
                }
            }

            if let Err(error) = request.send().await {
                tracing::error!(error = %error, action = action, "Discord fire-and-forget request failed");
            }
        });
    }

    #[instrument(skip(self, data), fields(method = ?method, path = path))]
    async fn make_request<T>(
        &self,
        method: Method,
        path: &str,
        data: Option<T>,
        headers: Option<HeaderMap>,
        endpoint: &str,
    ) -> DiscordResult<Response>
    where
        T: Serialize,
    {
        loop {
            if let Some((reset_time, wait)) = *self.global_rate_limit.read().await {
                if Instant::now() < reset_time {
                    tracing::warn!(wait_ms = wait.as_millis(), "Waiting for global rate limit");
                    sleep(reset_time - Instant::now()).await;
                }
            }

            if let Some(rate_limit) = self.rate_limits.get(endpoint) {
                if let Some(wait_time) = rate_limit.should_wait() {
                    tracing::warn!(
                        endpoint = endpoint,
                        wait_ms = wait_time.as_millis(),
                        "Waiting for endpoint rate limit"
                    );
                    sleep(wait_time).await;
                }
            }

            let url = self.build_url(path);
            let mut req = self
                .client
                .request(method.clone(), &url)
                .header("Authorization", self.token.as_ref());

            if let Some(ref data) = data {
                if let Ok(body) = serde_json::to_string(data) {
                    req = req.header("Content-Type", "application/json").body(body);
                }
            }

            if let Some(ref headers) = headers {
                for (key, value) in headers.iter() {
                    req = req.header(key, value);
                }
            }

            let response = req.send().await.map_err(|e| {
                tracing::error!(
                    error = %e,
                    url = url,
                    "Discord API request failed"
                );
                DiscordError::Middleware(e)
            })?;

            if response.status() == 429 {
                tracing::warn!(
                    endpoint = endpoint,
                    status = 429,
                    "Rate limited by Discord API"
                );
                if let Some(rate_limit) = RateLimit::from_headers(response.headers()) {
                    if rate_limit.is_global {
                        let wait_time = Duration::from_secs_f64(rate_limit.reset_after);
                        *self.global_rate_limit.write().await =
                            Some((Instant::now() + wait_time, wait_time));
                    } else {
                        self.rate_limits.insert(endpoint.to_string(), rate_limit);
                    }
                }
                continue;
            }

            if !response.status().is_success() {
                let status = response.status().as_u16();
                tracing::error!(
                    status = status,
                    endpoint = endpoint,
                    "Discord API request failed"
                );

                if let Err(error) = response.error_for_status_ref() {
                    tracing::error!(error = %error, endpoint = endpoint, "Discord API returned error status");
                }

                return Err(DiscordError::Other(USER_ERROR_DISCORD_REQUEST.into()));
            }

            let rate_limit = RateLimit::from_headers(response.headers());

            if let Some(rate_limit) = rate_limit {
                self.rate_limits.insert(endpoint.to_string(), rate_limit);
            }

            return Ok(response);
        }
    }

    #[instrument(skip(self))]
    pub async fn get_gateway(&self) -> DiscordResult<Gateway> {
        self.request_json(Method::GET, "/gateway", Option::<()>::None, None, "gateway")
            .await
    }

    #[instrument(skip(self), fields(channel_id = %channel_id))]
    pub async fn get_channel(&self, channel_id: &Id) -> DiscordResult<Channel> {
        self.request_json(
            Method::GET,
            &format!("/channels/{}", channel_id),
            Option::<()>::None,
            None,
            "channel",
        )
        .await
    }

    #[instrument(skip(self), fields(guild_id = %guild_id))]
    pub async fn get_guild(&self, guild_id: &Id) -> DiscordResult<Guild> {
        self.request_json(
            Method::GET,
            &format!("/guilds/{}", guild_id),
            Option::<()>::None,
            None,
            "guild",
        )
        .await
    }

    #[instrument(skip(self), fields(guild_id = %guild_id))]
    pub async fn get_guild_with_counts(&self, guild_id: &Id) -> DiscordResult<Guild> {
        self.request_json(
            Method::GET,
            &format!("/guilds/{}?with_counts=true", guild_id),
            Option::<()>::None,
            None,
            "guild",
        )
        .await
    }

    #[instrument(skip(self), fields(guild_id = %guild_id))]
    pub async fn get_guild_channels(&self, guild_id: &Id) -> DiscordResult<Vec<Channel>> {
        self.request_json(
            Method::GET,
            &format!("/guilds/{}/channels", guild_id),
            Option::<()>::None,
            None,
            "guild",
        )
        .await
    }

    #[instrument(skip(self), fields(guild_id = %guild_id))]
    pub async fn get_guild_members(&self, guild_id: &Id) -> DiscordResult<Vec<Member>> {
        self.request_json(
            Method::GET,
            &format!("/guilds/{}/members", guild_id),
            Option::<()>::None,
            None,
            "guild",
        )
        .await
    }

    #[instrument(skip(self), fields(guild_id = %guild_id, user_id = %user_id))]
    pub async fn get_member(&self, guild_id: &Id, user_id: &Id) -> DiscordResult<Member> {
        self.request_json(
            Method::GET,
            &format!("/guilds/{}/members/{}", guild_id, user_id),
            Option::<()>::None,
            None,
            "guild",
        )
        .await
    }

    #[instrument(skip(self), fields(guild_id = %guild_id))]
    pub async fn get_roles(&self, guild_id: &Id) -> DiscordResult<Vec<Role>> {
        self.request_json(
            Method::GET,
            &format!("/guilds/{}/roles", guild_id),
            Option::<()>::None,
            None,
            "guild",
        )
        .await
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn get_user(&self, user_id: &Id) -> DiscordResult<User> {
        self.request_json(
            Method::GET,
            &format!("/users/{}", user_id),
            Option::<()>::None,
            None,
            "user",
        )
        .await
    }

    #[instrument(skip(self))]
    pub async fn get_current_user(&self) -> DiscordResult<User> {
        self.request_json(Method::GET, "/users/@me", Option::<()>::None, None, "user")
            .await
    }

    #[instrument(skip(self, content), fields(channel_id = %channel_id))]
    pub async fn create_message(&self, channel_id: &Id, content: &str) -> DiscordResult<Message> {
        let data = json!({ "content": content });
        self.request_json(
            Method::POST,
            &Self::channel_messages_path(channel_id),
            Some(&data),
            None,
            "channel",
        )
        .await
    }

    #[instrument(skip(self, content), fields(channel_id = %channel_id, message_id = %message_id))]
    pub async fn edit_message(
        &self,
        channel_id: &Id,
        message_id: &Id,
        content: &str,
    ) -> DiscordResult<Message> {
        let data = json!({ "content": content });
        self.request_json(
            Method::PATCH,
            &Self::message_path(channel_id, message_id),
            Some(&data),
            None,
            "channel",
        )
        .await
    }

    #[instrument(skip(self, content), fields(channel_id = %channel_id))]
    pub async fn create_message_no_ping(
        &self,
        channel_id: &Id,
        content: &str,
    ) -> DiscordResult<Message> {
        let data = json!({ "content": content, "allowed_mentions": { "parse": [] } });
        self.request_json(
            Method::POST,
            &Self::channel_messages_path(channel_id),
            Some(&data),
            None,
            "channel",
        )
        .await
    }

    #[instrument(skip(self, content), fields(channel_id = %channel_id))]
    pub async fn create_message_and_forget(&self, channel_id: &Id, content: &str) {
        let data = json!({ "content": content });
        self.spawn_fire_and_forget(
            Method::POST,
            Self::channel_messages_path(channel_id),
            Some(data),
            "create_message",
        );
    }

    #[instrument(skip(self, embeds), fields(channel_id = %channel_id))]
    pub async fn create_message_with_embed(
        &self,
        channel_id: &Id,
        embeds: &[Embed],
    ) -> DiscordResult<Message> {
        let data = json!({ "embeds": embeds });
        self.request_json(
            Method::POST,
            &Self::channel_messages_path(channel_id),
            Some(&data),
            None,
            "channel",
        )
        .await
    }

    #[instrument(skip(self, embeds), fields(channel_id = %channel_id))]
    pub async fn create_message_with_embed_and_forget(&self, channel_id: &Id, embeds: &[Embed]) {
        let data = json!({ "embeds": embeds });
        self.spawn_fire_and_forget(
            Method::POST,
            Self::channel_messages_path(channel_id),
            Some(data),
            "create_message_embed",
        );
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn create_dm_channel(&self, user_id: &Id) -> DiscordResult<Id> {
        let data = json!({ "recipient_id": user_id });
        let channel: Value = self
            .request_json(
                Method::POST,
                "/users/@me/channels",
                Some(&data),
                None,
                "channel",
            )
            .await?;

        let Some(channel_id) = channel["id"].as_str() else {
            tracing::error!(payload = ?channel, "Discord DM channel payload missing id");
            return Err(DiscordError::Other(USER_ERROR_DISCORD_RESPONSE.into()));
        };

        channel_id.parse().map_err(|error| {
            tracing::error!(error = ?error, channel_id = channel_id, "Failed to parse Discord DM channel id");
            DiscordError::Other(USER_ERROR_DISCORD_RESPONSE.into())
        })
    }

    #[instrument(skip(self, reason), fields(guild_id = %guild_id, user_id = %user_id))]
    pub async fn kick_member(
        &self,
        guild_id: &Id,
        user_id: &Id,
        reason: Option<impl AsRef<str>>,
    ) -> DiscordResult<()> {
        let headers = Self::build_audit_log_headers(reason);

        self.request_empty(
            Method::DELETE,
            &format!("/guilds/{}/members/{}", guild_id, user_id),
            Option::<()>::None,
            headers,
            "guild",
        )
        .await
    }

    #[instrument(skip(self, reason), fields(guild_id = %guild_id, user_id = %user_id, delete_message_days = delete_message_days))]
    pub async fn ban_member(
        &self,
        guild_id: &Id,
        user_id: &Id,
        reason: Option<impl AsRef<str>>,
        delete_message_days: u8,
    ) -> DiscordResult<()> {
        let headers = Self::build_audit_log_headers(reason);

        let data = json!({ "delete_message_days": delete_message_days });
        self.request_empty(
            Method::PUT,
            &format!("/guilds/{}/bans/{}", guild_id, user_id),
            Some(&data),
            headers,
            "guild",
        )
        .await
    }

    #[instrument(skip(self, reason), fields(guild_id = %guild_id, user_id = %user_id))]
    pub async fn unban_member(
        &self,
        guild_id: &Id,
        user_id: &Id,
        reason: Option<impl AsRef<str>>,
    ) -> DiscordResult<()> {
        let headers = Self::build_audit_log_headers(reason);

        self.request_empty(
            Method::DELETE,
            &format!("/guilds/{}/bans/{}", guild_id, user_id),
            Option::<()>::None,
            headers,
            "guild",
        )
        .await
    }

    #[instrument(skip(self, reason), fields(guild_id = %guild_id, user_id = %user_id, role_id = %role_id))]
    pub async fn add_role(
        &self,
        guild_id: &Id,
        user_id: &Id,
        role_id: &Id,
        reason: Option<impl AsRef<str>>,
    ) -> DiscordResult<()> {
        let headers = Self::build_audit_log_headers(reason);

        self.request_empty(
            Method::PUT,
            &format!("/guilds/{}/members/{}/roles/{}", guild_id, user_id, role_id),
            Option::<()>::None,
            headers,
            "guild",
        )
        .await
    }

    #[instrument(skip(self, reason), fields(guild_id = %guild_id, user_id = %user_id, role_id = %role_id))]
    pub async fn remove_role(
        &self,
        guild_id: &Id,
        user_id: &Id,
        role_id: &Id,
        reason: Option<impl AsRef<str>>,
    ) -> DiscordResult<()> {
        let headers = Self::build_audit_log_headers(reason);

        self.request_empty(
            Method::DELETE,
            &format!("/guilds/{}/members/{}/roles/{}", guild_id, user_id, role_id),
            Option::<()>::None,
            headers,
            "guild",
        )
        .await
    }

    #[instrument(skip(self), fields(channel_id = %channel_id, message_id = %message_id))]
    pub async fn delete_message(&self, channel_id: &Id, message_id: &Id) -> DiscordResult<()> {
        self.request_empty(
            Method::DELETE,
            &Self::message_path(channel_id, message_id),
            Option::<()>::None,
            None,
            "channel",
        )
        .await
    }

    #[instrument(skip(self), fields(channel_id = %channel_id, message_id = %message_id))]
    pub async fn delete_message_and_forget(&self, channel_id: &Id, message_id: &Id) {
        self.spawn_fire_and_forget(
            Method::DELETE,
            Self::message_path(channel_id, message_id),
            None,
            "delete_message",
        );
    }
}
