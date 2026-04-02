pub(crate) mod memory;
pub(crate) mod redis;

use ::redis::{FromRedisValue, ToRedisArgs};
use async_trait::async_trait;
pub use memory::MemoryCache;
pub use redis::RedisCache;
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

pub use memory::MemoryCacheError;
pub use redis::RedisCacheError;

#[async_trait]
pub trait CacheBackend: Send + Sync {
    type Error;

    async fn get<K, V>(&self, key: &K) -> Result<Option<V>, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        V: DeserializeOwned;

    async fn set<K, V>(&self, key: K, value: &V, ttl: Option<Duration>) -> Result<(), Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        V: Serialize + Send + Sync;

    async fn incr<K>(&self, key: &K, ttl: Option<Duration>) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync;

    async fn incrby<K>(
        &self,
        key: &K,
        increment: u64,
        ttl: Option<Duration>,
    ) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync;

    async fn delete<K>(&self, key: &K) -> Result<(), Self::Error>
    where
        K: ToRedisArgs + Send + Sync;

    async fn exists<K>(&self, key: &K) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync;

    async fn zadd<K>(&self, key: &K, score: f64, member: &str) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync;

    async fn zremrangebyscore<K>(&self, key: &K, min: f64, max: f64) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync;

    async fn zcard<K>(&self, key: &K) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync;

    async fn expire<K>(&self, key: &K, ttl: Duration) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync;

    async fn sadd<K, M>(&self, key: &K, member: &M) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        M: ToRedisArgs + Send + Sync;

    async fn srem<K, M>(&self, key: &K, member: &M) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        M: ToRedisArgs + Send + Sync;

    async fn scard<K>(&self, key: &K) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync;

    async fn smembers<K, M>(&self, key: &K) -> Result<Vec<M>, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        M: DeserializeOwned + FromRedisValue;

    async fn ping(&self) -> Result<bool, Self::Error>;

    /// Returns all keys matching the given glob-style pattern.
    /// Uses a cursor-based scan on Redis; iterates the in-memory map otherwise.
    async fn scan(&self, pattern: &str) -> Result<Vec<String>, Self::Error>;

    /// Returns all keys matching the given glob-style pattern using `KEYS`.
    /// Prefer [`CacheBackend::scan`] in production; `keys` is provided for
    /// convenience in low-traffic / test scenarios.
    async fn keys(&self, pattern: &str) -> Result<Vec<String>, Self::Error>;
}

#[derive(Debug)]
pub struct Cache<B: CacheBackend> {
    backend: B,
}

impl<B: CacheBackend> Cache<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub async fn get<K, V>(&self, key: &K) -> Result<Option<V>, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
        V: DeserializeOwned,
    {
        self.backend.get(key).await
    }

    pub async fn set<K, V>(&self, key: K, value: &V, ttl: Option<Duration>) -> Result<(), B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
        V: Serialize + Send + Sync,
    {
        self.backend.set(key, value, ttl).await
    }

    pub async fn incr<K>(&self, key: &K, ttl: Option<Duration>) -> Result<u64, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
    {
        self.backend.incr(key, ttl).await
    }

    pub async fn delete<K>(&self, key: &K) -> Result<(), B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
    {
        self.backend.delete(key).await
    }

    pub async fn exists<K>(&self, key: &K) -> Result<bool, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
    {
        self.backend.exists(key).await
    }

    pub async fn zadd<K>(&self, key: &K, score: f64, member: &str) -> Result<bool, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
    {
        self.backend.zadd(key, score, member).await
    }

    pub async fn zremrangebyscore<K>(&self, key: &K, min: f64, max: f64) -> Result<u64, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
    {
        self.backend.zremrangebyscore(key, min, max).await
    }

    pub async fn zcard<K>(&self, key: &K) -> Result<u64, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
    {
        self.backend.zcard(key).await
    }

    pub async fn expire<K>(&self, key: &K, ttl: Duration) -> Result<bool, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
    {
        self.backend.expire(key, ttl).await
    }

    pub async fn sadd<K, M>(&self, key: &K, member: &M) -> Result<bool, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
        M: ToRedisArgs + Send + Sync,
    {
        self.backend.sadd(key, member).await
    }

    pub async fn srem<K, M>(&self, key: &K, member: &M) -> Result<bool, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
        M: ToRedisArgs + Send + Sync,
    {
        self.backend.srem(key, member).await
    }

    pub async fn scard<K>(&self, key: &K) -> Result<u64, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
    {
        self.backend.scard(key).await
    }

    pub async fn smembers<K, M>(&self, key: &K) -> Result<Vec<M>, B::Error>
    where
        K: ToString + ToRedisArgs + Send + Sync,
        M: DeserializeOwned + FromRedisValue,
    {
        self.backend.smembers(key).await
    }

    pub async fn ping(&self) -> Result<bool, B::Error> {
        self.backend.ping().await
    }

    pub async fn scan(&self, pattern: &str) -> Result<Vec<String>, B::Error> {
        self.backend.scan(pattern).await
    }

    pub async fn keys(&self, pattern: &str) -> Result<Vec<String>, B::Error> {
        self.backend.keys(pattern).await
    }
}
