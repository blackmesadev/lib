use super::CacheBackend;
use async_trait::async_trait;
use dashmap::DashMap;
use redis::{FromRedisValue, ToRedisArgs};
use serde::{de::DeserializeOwned, Serialize};
use serde_json;
use std::collections::{BTreeMap, Vec};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct CacheEntry {
    data: Vec<u8>,
    expires_at: Option<Instant>,
    zset: Option<BTreeMap<String, f64>>,
    set: Option<Vec<String>>,
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryCacheError {
    Serialization(#[from] serde_json::Error),
}

impl std::fmt::Display for MemoryCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Default)]
pub struct MemoryCache {
    data: DashMap<String, CacheEntry>,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn is_expired(entry: &CacheEntry) -> bool {
        entry
            .expires_at
            .map(|expires| expires <= Instant::now())
            .unwrap_or(false)
    }

    fn get_entry(&self, key: &str) -> Option<dashmap::mapref::one::Ref<'_, String, CacheEntry>> {
        let entry = self.data.get(key)?;
        if Self::is_expired(&entry) {
            let key = key.to_string();
            drop(entry);
            self.data.remove(&key);
            None
        } else {
            Some(entry)
        }
    }

    fn get_entry_mut(
        &self,
        key: &str,
    ) -> Option<dashmap::mapref::one::RefMut<'_, String, CacheEntry>> {
        let entry = self.data.get_mut(key)?;
        if Self::is_expired(&entry) {
            let key = key.to_string();
            drop(entry);
            self.data.remove(&key);
            None
        } else {
            Some(entry)
        }
    }

    fn key_to_string<K: ToRedisArgs>(key: &K) -> String {
        let mut args = vec![];
        key.write_redis_args(&mut args);
        String::from_utf8_lossy(&args.concat()).into_owned()
    }
}

#[async_trait]
impl CacheBackend for MemoryCache {
    type Error = MemoryCacheError;

    async fn get<K, V>(&self, key: &K) -> Result<Option<V>, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        V: DeserializeOwned,
    {
        let key = Self::key_to_string(key);

        let result = match self.get_entry(&key) {
            Some(entry) => {
                let value: V = serde_json::from_slice(&entry.data)?;
                Ok(Some(value))
            }
            None => Ok(None),
        };

        result
    }

    async fn set<K, V>(&self, key: K, value: &V, ttl: Option<Duration>) -> Result<(), Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        V: Serialize + Send + Sync,
    {
        let key = Self::key_to_string(&key);

        let data = match serde_json::to_vec(value) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to serialize value");
                return Err(e.into());
            }
        };

        let expires_at = ttl.map(|duration| Instant::now() + duration);
        self.data.insert(
            key.to_string(),
            CacheEntry {
                data,
                expires_at,
                zset: None,
                set: None,
            },
        );

        Ok(())
    }

    async fn incr<K>(&self, key: &K, ttl: Option<Duration>) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);

        let value = match self.get_entry_mut(&key) {
            Some(mut entry) => {
                let value = match serde_json::from_slice::<u64>(&entry.data) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!(error = ?e, "Failed to deserialize value");
                        return Err(e.into());
                    }
                };

                let new_value = value + 1;
                entry.data = serde_json::to_vec(&new_value)?;
                entry.expires_at = ttl.map(|duration| Instant::now() + duration);

                new_value
            }
            None => {
                self.set(key, &1u64, ttl).await?;
                1
            }
        };

        Ok(value)
    }

    async fn incrby<K>(
        &self,
        key: &K,
        increment: u64,
        ttl: Option<Duration>,
    ) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);

        let value = match self.get_entry_mut(&key) {
            Some(mut entry) => {
                let value = match serde_json::from_slice::<u64>(&entry.data) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!(error = ?e, "Failed to deserialize value");
                        return Err(e.into());
                    }
                };

                let new_value = value + increment;
                entry.data = serde_json::to_vec(&new_value)?;
                entry.expires_at = ttl.map(|duration| Instant::now() + duration);

                new_value
            }
            None => {
                self.set(key, &increment, ttl).await?;
                increment
            }
        };

        Ok(value)
    }

    async fn delete<K>(&self, key: &K) -> Result<(), Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);
        self.data.remove(&key);
        Ok(())
    }

    async fn exists<K>(&self, key: &K) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);

        if let Some(entry) = self.data.get(&key) {
            if Self::is_expired(&entry) {
                self.data.remove(&key);
                return Ok(false);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn zadd<K>(&self, key: &K, score: f64, member: &str) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);
        let mut entry = self.data.entry(key).or_insert_with(|| CacheEntry {
            data: Vec::new(),
            expires_at: None,
            zset: Some(BTreeMap::new()),
            set: None,
        });

        let zset = entry.zset.get_or_insert_with(BTreeMap::new);
        let is_new = !zset.contains_key(member);
        zset.insert(member.to_string(), score);
        Ok(is_new)
    }

    async fn sadd<K, M>(&self, key: &K, member: &M) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        M: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);
        let member_str = Self::key_to_string(member);
        let mut entry = self.data.entry(key).or_insert_with(|| CacheEntry {
            data: Vec::new(),
            expires_at: None,
            zset: None,
            set: Some(Vec::new()),
        });

        let set = entry.set.get_or_insert_with(Vec::new);
        Ok(set.insert(member_str))
    }

    async fn srem<K, M>(&self, key: &K, member: &M) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        M: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);
        let member_str = Self::key_to_string(member);
        if let Some(mut entry) = self.data.get_mut(&key) {
            if let Some(set) = &mut entry.set {
                return Ok(set.remove(&member_str));
            }
        }
        Ok(false)
    }

    async fn scard<K>(&self, key: &K) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);
        if let Some(entry) = self.get_entry(&key) {
            if let Some(set) = &entry.set {
                return Ok(set.len() as u64);
            }
        }
        Ok(0)
    }

    async fn smembers<K, M>(&self, key: &K) -> Result<Vec<M>, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
        M: DeserializeOwned + FromRedisValue,
    {
        let key = Self::key_to_string(key);
        if let Some(entry) = self.get_entry(&key) {
            if let Some(set) = &entry.set {
                let mut result = Vec::with_capacity(set.len());
                for member_str in set {
                    result.push(serde_json::from_str(member_str)?);
                }
                return Ok(result);
            }
        }
        Ok(Vec::new())
    }

    async fn zremrangebyscore<K>(&self, key: &K, min: f64, max: f64) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);
        if let Some(mut entry) = self.data.get_mut(&key) {
            if let Some(zset) = &mut entry.zset {
                let before_len = zset.len();
                zset.retain(|_, score| *score < min || *score > max);
                return Ok((before_len - zset.len()) as u64);
            }
        }
        Ok(0)
    }

    async fn zcard<K>(&self, key: &K) -> Result<u64, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);
        if let Some(entry) = self.get_entry(&key) {
            if let Some(zset) = &entry.zset {
                return Ok(zset.len() as u64);
            }
        }
        Ok(0)
    }

    async fn expire<K>(&self, key: &K, ttl: Duration) -> Result<bool, Self::Error>
    where
        K: ToRedisArgs + Send + Sync,
    {
        let key = Self::key_to_string(key);
        if let Some(mut entry) = self.data.get_mut(&key) {
            entry.expires_at = Some(Instant::now() + ttl);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn ping(&self) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn scan(&self, pattern: &str) -> Result<Vec<String>, Self::Error> {
        self.keys(pattern).await
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>, Self::Error> {
        Ok(self
            .data
            .iter()
            .filter(|e| !Self::is_expired(e.value()) && glob_match(pattern, e.key()))
            .map(|e| e.key().clone())
            .collect())
    }
}

/// Minimal glob matching supporting only `*` as a wildcard (zero or more of
/// any character).  Sufficient for patterns like `roles:*:12345`.
fn glob_match(pattern: &str, s: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == s;
    }
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            // first segment must be a prefix
            if !s.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            // last segment must be a suffix
            return s[pos..].ends_with(part);
        } else {
            match s[pos..].find(part) {
                Some(off) => pos += off + part.len(),
                None => return false,
            }
        }
    }
    true
}
