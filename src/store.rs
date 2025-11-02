use redis::{Client, Commands, RedisResult};
use serde_json::Value;

/// The key prefix for items in Redis
const ITEMS_KEY: &str = "items";

/// Store provides an abstract interface for CRUD operations on Redis
#[derive(Clone)]
pub struct Store {
    pub redis_url: String,
}

impl Store {
    pub fn new(redis_url: String) -> Self {
        Self { redis_url }
    }

    fn get_redis_client(&self) -> RedisResult<Client> {
        redis::Client::open(self.redis_url.clone())
    }

    /// Create a new item in the store
    /// Items are stored as JSON strings in a Redis set
    pub fn create(&self, item: &Value) -> RedisResult<()> {
        let client = self.get_redis_client()?;
        let mut con = client.get_connection()?;

        let payload = serde_json::to_string(item).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Failed to serialize JSON",
                format!("{}", e),
            ))
        })?;

        let _added: i32 = con.sadd(ITEMS_KEY, payload)?;
        Ok(())
    }

    /// Read (retrieve and remove) an item from the store
    /// This pops one item from the set
    pub fn read(&self) -> RedisResult<Value> {
        let client = self.get_redis_client()?;
        let mut con = client.get_connection()?;

        let raw: Option<String> = con.spop(ITEMS_KEY)?;

        match raw {
            Some(s) => serde_json::from_str::<Value>(&s).map_err(|e| {
                redis::RedisError::from((
                    redis::ErrorKind::TypeError,
                    "Stored value is not valid JSON",
                    format!("{}", e),
                ))
            }),
            None => Err(redis::RedisError::from((
                redis::ErrorKind::ResponseError,
                "No items available",
            ))),
        }
    }

    /// List all items in the store without removing them
    pub fn list(&self) -> RedisResult<Vec<Value>> {
        let client = self.get_redis_client()?;
        let mut con = client.get_connection()?;

        let raw_items: Vec<String> = con.smembers(ITEMS_KEY)?;

        let mut items = Vec::new();
        for raw in raw_items {
            let value = serde_json::from_str::<Value>(&raw).map_err(|e| {
                redis::RedisError::from((
                    redis::ErrorKind::TypeError,
                    "Stored value is not valid JSON",
                    format!("{}", e),
                ))
            })?;
            items.push(value);
        }

        Ok(items)
    }

    /// Update is implemented as remove + add
    /// In a real system, you'd use Redis hashes with IDs for true updates
    /// This is kept simple for template purposes
    pub fn update(&self, old_item: &Value, new_item: &Value) -> RedisResult<()> {
        let client = self.get_redis_client()?;
        let mut con = client.get_connection()?;

        let old_payload = serde_json::to_string(old_item).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Failed to serialize old JSON",
                format!("{}", e),
            ))
        })?;

        let new_payload = serde_json::to_string(new_item).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Failed to serialize new JSON",
                format!("{}", e),
            ))
        })?;

        let removed: i32 = con.srem(ITEMS_KEY, &old_payload)?;
        if removed == 0 {
            return Err(redis::RedisError::from((
                redis::ErrorKind::ResponseError,
                "Item not found",
            )));
        }

        let _added: i32 = con.sadd(ITEMS_KEY, new_payload)?;
        Ok(())
    }

    /// Delete an item from the store
    pub fn delete(&self, item: &Value) -> RedisResult<()> {
        let client = self.get_redis_client()?;
        let mut con = client.get_connection()?;

        let payload = serde_json::to_string(item).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Failed to serialize JSON",
                format!("{}", e),
            ))
        })?;

        let removed: i32 = con.srem(ITEMS_KEY, payload)?;
        if removed == 0 {
            return Err(redis::RedisError::from((
                redis::ErrorKind::ResponseError,
                "Item not found",
            )));
        }

        Ok(())
    }
}
