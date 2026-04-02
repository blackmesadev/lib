use super::CacheBackend;
use async_trait::async_trait;
use redis::{FromRedisValue, ToRedisArgs, aio::MultiplexedConnection};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum RedisCacheError {
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Lazily prefixes a key at the redis wire layer — no allocation until
/// `write_redis_args` is called.
struct Prefixed<'a, K: ToRedisArgs> {
    prefix: &'a [u8], // already includes the trailing ':'
    key: &'a K,
}

/// Intercepts each arg the wrapped key emits and prepends the prefix.
/// Reuses `buf` across multiple args (one allocation for the whole call).
struct PrefixWriter<'a, W: ?Sized + redis::RedisWrite> {
    buf: Vec<u8>,
    prefix_end: usize,
    out: &'a mut W,
}

impl<W: ?Sized + redis::RedisWrite> redis::RedisWrite for PrefixWriter<'_, W> {
    fn write_arg(&mut self, arg: &[u8]) {
        self.buf.truncate(self.prefix_end);
        self.buf.extend_from_slice(arg);
        self.out.write_arg(&self.buf);
    }

    fn write_arg_fmt(&mut self, arg: impl std::fmt::Display) {
        self.write_arg(arg.to_string().as_bytes());
    }

    fn writer_for_next_arg(&mut self) -> impl std::io::Write + '_ {
        self.buf.truncate(self.prefix_end);
        PrefixCommit {
            buf: &mut self.buf,
            out: &mut *self.out,
        }
    }
}

/// Streaming counterpart to `PrefixWriter::write_arg`: accumulates bytes into
/// `buf` then commits them on drop.
struct PrefixCommit<'a, W: ?Sized + redis::RedisWrite> {
    buf: &'a mut Vec<u8>,
    out: &'a mut W,
}

impl<W: ?Sized + redis::RedisWrite> std::io::Write for PrefixCommit<'_, W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<W: ?Sized + redis::RedisWrite> Drop for PrefixCommit<'_, W> {
    fn drop(&mut self) {
        self.out.write_arg(self.buf);
    }
}

impl<'a, K: ToRedisArgs> ToRedisArgs for Prefixed<'a, K> {
    fn write_redis_args<W: ?Sized + redis::RedisWrite>(&self, out: &mut W) {
        let mut w = PrefixWriter {
            buf: {
                let mut v = Vec::with_capacity(self.prefix.len() + 64);
                v.extend_from_slice(self.prefix);
                v
            },
            prefix_end: self.prefix.len(),
            out,
        };
        self.key.write_redis_args(&mut w);
    }
}

#[derive(Debug, Clone)]
pub struct RedisCache {
    conn: MultiplexedConnection,
    /// Prefix bytes with trailing `:`  e.g. `b"myapp:"`
    prefix: Box<[u8]>,
}

impl RedisCache {
    pub async fn new(url: String, prefix: String) -> Result<Self, RedisCacheError> {
        let conn = match redis::Client::open(url) {
            Ok(client) => match client.get_multiplexed_async_connection().await {
                Ok(conn) => conn,
                Err(error) => {
                    tracing::error!(error = %error, "Failed to connect to Redis");
                    return Err(error.into());
                }
            },
            Err(error) => {
                tracing::error!(error = %error, "Failed to initialize Redis client");
                return Err(error.into());
            }
        };

        let mut prefix_bytes = prefix.into_bytes();
        prefix_bytes.push(b':');

        Ok(Self {
            conn,
            prefix: prefix_bytes.into_boxed_slice(),
        })
    }

    fn pk<'a, K: ToRedisArgs>(&'a self, key: &'a K) -> Prefixed<'a, K> {
        Prefixed {
            prefix: &self.prefix,
            key,
        }
    }
}

#[async_trait]
impl CacheBackend for RedisCache {
    type Error = RedisCacheError;

    async fn set<K, V>(&self, key: K, value: &V, ttl: Option<Duration>) -> Result<(), Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        V: Serialize + Send + Sync,
    {
        let start = std::time::Instant::now();
        let value = match serde_json::to_vec(value) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to serialize value");
                return Err(e.into());
            }
        };

        let mut conn = self.conn.clone();
        let result = if let Some(ttl) = ttl {
            redis::cmd("SETEX")
                .arg(self.pk(&key))
                .arg(ttl.as_secs() as usize)
                .arg(value)
                .exec_async(&mut conn)
                .await
        } else {
            redis::cmd("SET")
                .arg(self.pk(&key))
                .arg(value)
                .exec_async(&mut conn)
                .await
        };

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::error!(error = ?e, duration_ms = start.elapsed().as_millis(), "Redis operation failed");
                Err(e.into())
            }
        }
    }

    async fn get<K, V>(&self, key: &K) -> Result<Option<V>, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        V: DeserializeOwned,
    {
        let mut conn = self.conn.clone();
        let result: Option<Vec<u8>> = match redis::cmd("GET")
            .arg(self.pk(key))
            .query_async(&mut conn)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(error = %error, "Redis GET operation failed");
                return Err(error.into());
            }
        };

        match result {
            Some(v) => match serde_json::from_slice(&v) {
                Ok(value) => Ok(Some(value)),
                Err(error) => {
                    tracing::error!(error = %error, "Failed to deserialize Redis value");
                    Err(error.into())
                }
            },
            None => Ok(None),
        }
    }

    async fn incr<K>(&self, key: &K, ttl: Option<Duration>) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let value: u64 = if let Some(ttl) = ttl {
            let (v, _): (u64, bool) = match redis::pipe()
                .cmd("INCR")
                .arg(self.pk(key))
                .cmd("EXPIRE")
                .arg(self.pk(key))
                .arg(ttl.as_secs() as usize)
                .query_async(&mut conn)
                .await
            {
                Ok(v) => v,
                Err(error) => {
                    tracing::error!(error = %error, "Redis INCR+EXPIRE pipeline failed");
                    return Err(error.into());
                }
            };
            v
        } else {
            match redis::cmd("INCR")
                .arg(self.pk(key))
                .query_async(&mut conn)
                .await
            {
                Ok(v) => v,
                Err(error) => {
                    tracing::error!(error = %error, "Redis INCR operation failed");
                    return Err(error.into());
                }
            }
        };

        Ok(value)
    }

    async fn incrby<K>(
        &self,
        key: &K,
        value: u64,
        ttl: Option<Duration>,
    ) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let next_value: u64 = if let Some(ttl) = ttl {
            let (v, _): (u64, bool) = match redis::pipe()
                .cmd("INCRBY")
                .arg(self.pk(key))
                .arg(value)
                .cmd("EXPIRE")
                .arg(self.pk(key))
                .arg(ttl.as_secs() as usize)
                .query_async(&mut conn)
                .await
            {
                Ok(v) => v,
                Err(error) => {
                    tracing::error!(error = %error, "Redis INCRBY+EXPIRE pipeline failed");
                    return Err(error.into());
                }
            };
            v
        } else {
            match redis::cmd("INCRBY")
                .arg(self.pk(key))
                .arg(value)
                .query_async(&mut conn)
                .await
            {
                Ok(v) => v,
                Err(error) => {
                    tracing::error!(error = %error, "Redis INCRBY operation failed");
                    return Err(error.into());
                }
            }
        };

        Ok(next_value)
    }

    async fn delete<K>(&self, key: &K) -> Result<(), Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        if let Err(error) = redis::cmd("DEL")
            .arg(self.pk(key))
            .exec_async(&mut conn)
            .await
        {
            tracing::error!(error = %error, "Redis DEL operation failed");
            return Err(error.into());
        }

        Ok(())
    }

    async fn exists<K>(&self, key: &K) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let exists: bool = match redis::cmd("EXISTS")
            .arg(self.pk(key))
            .query_async(&mut conn)
            .await
        {
            Ok(exists) => exists,
            Err(error) => {
                tracing::error!(error = %error, "Redis EXISTS operation failed");
                return Err(error.into());
            }
        };

        Ok(exists)
    }

    async fn zadd<K>(&self, key: &K, score: f64, member: &str) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let added: bool = match redis::cmd("ZADD")
            .arg(self.pk(key))
            .arg(score)
            .arg(member)
            .query_async(&mut conn)
            .await
        {
            Ok(added) => added,
            Err(error) => {
                tracing::error!(error = %error, "Redis ZADD operation failed");
                return Err(error.into());
            }
        };

        Ok(added)
    }

    async fn zremrangebyscore<K>(&self, key: &K, min: f64, max: f64) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let removed: u64 = match redis::cmd("ZREMRANGEBYSCORE")
            .arg(self.pk(key))
            .arg(min)
            .arg(max)
            .query_async(&mut conn)
            .await
        {
            Ok(removed) => removed,
            Err(error) => {
                tracing::error!(error = %error, "Redis ZREMRANGEBYSCORE operation failed");
                return Err(error.into());
            }
        };

        Ok(removed)
    }

    async fn zcard<K>(&self, key: &K) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let count: u64 = match redis::cmd("ZCARD")
            .arg(self.pk(key))
            .query_async(&mut conn)
            .await
        {
            Ok(count) => count,
            Err(error) => {
                tracing::error!(error = %error, "Redis ZCARD operation failed");
                return Err(error.into());
            }
        };

        Ok(count)
    }

    async fn expire<K>(&self, key: &K, ttl: Duration) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let set: bool = match redis::cmd("EXPIRE")
            .arg(self.pk(key))
            .arg(ttl.as_secs() as usize)
            .query_async(&mut conn)
            .await
        {
            Ok(set) => set,
            Err(error) => {
                tracing::error!(error = %error, "Redis EXPIRE operation failed");
                return Err(error.into());
            }
        };

        Ok(set)
    }

    async fn sadd<K, M>(&self, key: &K, member: &M) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        M: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let added: u64 = match redis::cmd("SADD")
            .arg(self.pk(key))
            .arg(member)
            .query_async(&mut conn)
            .await
        {
            Ok(added) => added,
            Err(error) => {
                tracing::error!(error = %error, "Redis SADD operation failed");
                return Err(error.into());
            }
        };

        Ok(added > 0)
    }

    async fn srem<K, M>(&self, key: &K, member: &M) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        M: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let removed: u64 = match redis::cmd("SREM")
            .arg(self.pk(key))
            .arg(member)
            .query_async(&mut conn)
            .await
        {
            Ok(removed) => removed,
            Err(error) => {
                tracing::error!(error = %error, "Redis SREM operation failed");
                return Err(error.into());
            }
        };

        Ok(removed > 0)
    }

    async fn scard<K>(&self, key: &K) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let mut conn = self.conn.clone();

        let count: u64 = match redis::cmd("SCARD")
            .arg(self.pk(key))
            .query_async(&mut conn)
            .await
        {
            Ok(count) => count,
            Err(error) => {
                tracing::error!(error = %error, "Redis SCARD operation failed");
                return Err(error.into());
            }
        };

        Ok(count)
    }

    async fn smembers<K, M>(&self, key: &K) -> Result<Vec<M>, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        M: DeserializeOwned + FromRedisValue,
    {
        let mut conn = self.conn.clone();

        let members: Vec<M> = match redis::cmd("SMEMBERS")
            .arg(self.pk(key))
            .query_async(&mut conn)
            .await
        {
            Ok(members) => members,
            Err(error) => {
                tracing::error!(error = %error, "Redis SMEMBERS operation failed");
                return Err(error.into());
            }
        };

        Ok(members)
    }

    async fn ping(&self) -> Result<bool, Self::Error> {
        let mut conn = self.conn.clone();

        match redis::cmd("PING").query_async::<String>(&mut conn).await {
            Ok(response) => Ok(response == "PONG"),
            Err(error) => {
                tracing::error!(error = %error, "Redis PING operation failed");
                Err(error.into())
            }
        }
    }

    async fn scan(&self, pattern: &str) -> Result<Vec<String>, Self::Error> {
        let prefix_str = std::str::from_utf8(&self.prefix).unwrap_or_default();
        let full_pattern = format!("{}{}", prefix_str, pattern);
        let prefix_len = self.prefix.len();
        let mut conn = self.conn.clone();
        let mut cursor: u64 = 0;
        let mut results = Vec::new();
        loop {
            let (next_cursor, batch): (u64, Vec<String>) = match redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&full_pattern)
                .arg("COUNT")
                .arg(100u64)
                .query_async(&mut conn)
                .await
            {
                Ok(v) => v,
                Err(error) => {
                    tracing::error!(error = %error, "Redis SCAN operation failed");
                    return Err(error.into());
                }
            };
            results.extend(
                batch
                    .into_iter()
                    .filter_map(|k| k.get(prefix_len..).map(str::to_owned)),
            );
            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }
        Ok(results)
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>, Self::Error> {
        let prefix_str = std::str::from_utf8(&self.prefix).unwrap_or_default();
        let full_pattern = format!("{}{}", prefix_str, pattern);
        let prefix_len = self.prefix.len();
        let mut conn = self.conn.clone();
        let raw: Vec<String> = match redis::cmd("KEYS")
            .arg(&full_pattern)
            .query_async(&mut conn)
            .await
        {
            Ok(v) => v,
            Err(error) => {
                tracing::error!(error = %error, "Redis KEYS operation failed");
                return Err(error.into());
            }
        };
        Ok(raw
            .into_iter()
            .filter_map(|k| k.get(prefix_len..).map(str::to_owned))
            .collect())
    }
}
