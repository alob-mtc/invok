use redis::{aio::MultiplexedConnection, AsyncCommands, ExistenceCheck, SetExpiry, SetOptions};
use tracing::error;

pub struct FunctionCacheRepo;

impl FunctionCacheRepo {
    /// Retrieves the cached function address by its name.
    ///
    /// # Arguments
    ///
    /// * `conn` - A mutable reference to the Redis connection.
    /// * `name` - The key representing the function.
    ///
    /// # Returns
    ///
    /// * `Some(String)` containing the cached address if found, or `None` if not found or an error occurs.
    pub async fn get_function(conn: &mut MultiplexedConnection, name: &str) -> Option<()> {
        match conn.exists::<&str, usize>(name).await{
            Ok(_) => Some(()),
            Err(e) => {
                error!("Failed to retrieve function '{}' from cache: {}", name, e);
                None
            }
        }
    }

    /// Adds a function address to the cache with a specified time-to-live (TTL).
    ///
    /// The entry is only added if it does not already exist.
    ///
    /// # Arguments
    ///
    /// * `conn` - A mutable reference to the Redis connection.
    /// * `name` - The key representing the function.
    /// * `ttl` - Time-to-live in seconds.
    ///
    /// # Returns
    ///
    /// * `Ok(())` on success, or a `redis::RedisError` if the operation fails.
    pub async fn add_function(
        conn: &mut MultiplexedConnection,
        name: &str,
        ttl: u64,
    ) -> redis::RedisResult<()> {
        let opts = SetOptions::default()
            .conditional_set(ExistenceCheck::NX)
            .get(true)
            .with_expiration(SetExpiry::EX(ttl));
        conn.set_options(name, 1, opts).await.map_err(|e| {
            error!("Failed to add function '{}' to cache: {}", name, e);
            e
        })
    }
}
