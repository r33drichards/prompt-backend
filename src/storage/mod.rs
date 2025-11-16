//! Storage abstraction layer for file operations
//!
//! This module provides a pluggable storage interface that allows swapping
//! between different storage backends (local filesystem, S3, etc.) without
//! changing application code.
//!
//! # Architecture
//!
//! The storage layer follows a trait-based design pattern:
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │         Application Code                │
//! │    (handlers, background tasks)         │
//! └──────────────┬──────────────────────────┘
//!                │
//!                │ Uses StorageBackend trait
//!                ▼
//! ┌──────────────────────────────────────────┐
//! │       StorageBackend Trait               │
//! │  (put, get, exists, delete, list, etc.)  │
//! └──────────────┬───────────────────────────┘
//!                │
//!     ┌──────────┴──────────┬────────────┐
//!     ▼                     ▼            ▼
//! ┌─────────┐          ┌─────────┐  ┌─────────┐
//! │  Local  │          │   S3    │  │  Other  │
//! │ Storage │          │ Storage │  │ Backends│
//! └─────────┘          └─────────┘  └─────────┘
//! ```
//!
//! # Example Usage
//!
//! ## Using the Factory
//!
//! ```no_run
//! use prompt_backend::storage::{StorageFactory, StorageConfig, StorageBackend};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create from environment variables
//!     let storage = StorageFactory::from_env()?;
//!     
//!     // Or create with explicit configuration
//!     let config = StorageConfig::Local {
//!         base_path: "/var/app/storage".to_string()
//!     };
//!     let storage = StorageFactory::create(config)?;
//!     
//!     // Use the storage backend
//!     storage.put("images/photo.jpg", vec![1, 2, 3], None).await?;
//!     let data = storage.get("images/photo.jpg", None).await?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Direct Backend Usage
//!
//! ```no_run
//! use prompt_backend::storage::{LocalStorage, S3Storage, StorageBackend};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Local storage
//!     let local = LocalStorage::new("/tmp/storage")?;
//!     local.put("file.txt", b"data".to_vec(), None).await?;
//!     
//!     // S3 storage
//!     let s3 = S3Storage::new(
//!         "my-bucket".to_string(),
//!         "us-east-1".to_string(),
//!         None,
//!         None,
//!         None,
//!     )?;
//!     s3.put("file.txt", b"data".to_vec(), None).await?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! # Environment Configuration
//!
//! The storage layer can be configured via environment variables:
//!
//! ## Local Storage
//! ```bash
//! STORAGE_TYPE=local
//! STORAGE_BASE_PATH=/var/app/storage
//! ```
//!
//! ## S3 Storage
//! ```bash
//! STORAGE_TYPE=s3
//! S3_BUCKET=my-bucket
//! S3_REGION=us-east-1
//! S3_ACCESS_KEY=AKIAIOSFODNN7EXAMPLE  # Optional
//! S3_SECRET_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY  # Optional
//! S3_ENDPOINT=https://nyc3.digitaloceanspaces.com  # Optional, for S3-compatible services
//! ```

mod local;
mod s3;
mod traits;

pub use local::LocalStorage;
pub use s3::S3Storage;
pub use traits::{
    FileMetadata, GetOptions, PutOptions, StorageBackend, StorageConfig, StorageError,
    StorageResult,
};

use std::sync::Arc;

/// Factory for creating storage backends based on configuration
///
/// This factory provides convenient methods to create storage backends from
/// configuration structs or environment variables.
pub struct StorageFactory;

impl StorageFactory {
    /// Create a storage backend from configuration
    ///
    /// # Arguments
    /// * `config` - Storage configuration
    ///
    /// # Returns
    /// An Arc-wrapped storage backend implementing the StorageBackend trait
    ///
    /// # Example
    /// ```no_run
    /// use prompt_backend::storage::{StorageFactory, StorageConfig};
    ///
    /// let config = StorageConfig::Local {
    ///     base_path: "/tmp/storage".to_string()
    /// };
    /// let storage = StorageFactory::create(config)?;
    /// # Ok::<(), prompt_backend::storage::StorageError>(())
    /// ```
    pub fn create(config: StorageConfig) -> StorageResult<Arc<dyn StorageBackend>> {
        match config {
            StorageConfig::Local { base_path } => {
                let storage = LocalStorage::new(base_path)?;
                Ok(Arc::new(storage))
            }
            StorageConfig::S3 {
                bucket,
                region,
                access_key,
                secret_key,
                endpoint,
            } => {
                let storage = S3Storage::new(bucket, region, access_key, secret_key, endpoint)?;
                Ok(Arc::new(storage))
            }
        }
    }

    /// Create storage from environment variables
    ///
    /// Reads configuration from:
    /// - `STORAGE_TYPE`: "local" or "s3" (default: "local")
    /// - For local: `STORAGE_BASE_PATH` (default: "/tmp/storage")
    /// - For S3:
    ///   - `S3_BUCKET` (required)
    ///   - `S3_REGION` (default: "us-east-1")
    ///   - `S3_ACCESS_KEY` (optional, uses AWS credentials chain if not set)
    ///   - `S3_SECRET_KEY` (optional)
    ///   - `S3_ENDPOINT` (optional, for S3-compatible services)
    ///
    /// # Returns
    /// An Arc-wrapped storage backend implementing the StorageBackend trait
    ///
    /// # Example
    /// ```no_run
    /// use prompt_backend::storage::StorageFactory;
    ///
    /// // Reads from environment: STORAGE_TYPE, STORAGE_BASE_PATH, etc.
    /// let storage = StorageFactory::from_env()?;
    /// # Ok::<(), prompt_backend::storage::StorageError>(())
    /// ```
    pub fn from_env() -> StorageResult<Arc<dyn StorageBackend>> {
        let storage_type = std::env::var("STORAGE_TYPE").unwrap_or_else(|_| "local".to_string());

        match storage_type.to_lowercase().as_str() {
            "local" => {
                let base_path = std::env::var("STORAGE_BASE_PATH")
                    .unwrap_or_else(|_| "/tmp/storage".to_string());
                Self::create(StorageConfig::Local { base_path })
            }
            "s3" => {
                let bucket = std::env::var("S3_BUCKET").map_err(|_| {
                    StorageError::Config("S3_BUCKET environment variable required".to_string())
                })?;
                let region =
                    std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
                let access_key = std::env::var("S3_ACCESS_KEY").ok();
                let secret_key = std::env::var("S3_SECRET_KEY").ok();
                let endpoint = std::env::var("S3_ENDPOINT").ok();

                Self::create(StorageConfig::S3 {
                    bucket,
                    region,
                    access_key,
                    secret_key,
                    endpoint,
                })
            }
            _ => Err(StorageError::Config(format!(
                "Unknown storage type: {}. Supported: local, s3",
                storage_type
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_local_storage() {
        let config = StorageConfig::Local {
            base_path: "/tmp/test".to_string(),
        };
        let storage = StorageFactory::create(config);
        assert!(storage.is_ok());
    }

    #[test]
    fn test_from_env_default() {
        // Should default to local storage
        std::env::remove_var("STORAGE_TYPE");
        let storage = StorageFactory::from_env();
        assert!(storage.is_ok());
        assert_eq!(storage.unwrap().backend_name(), "local");
    }

    #[test]
    fn test_from_env_s3_missing_bucket() {
        std::env::set_var("STORAGE_TYPE", "s3");
        std::env::remove_var("S3_BUCKET");
        let storage = StorageFactory::from_env();
        assert!(storage.is_err());
        std::env::remove_var("STORAGE_TYPE");
    }

    #[test]
    fn test_backend_name() {
        let local_config = StorageConfig::Local {
            base_path: "/tmp/test".to_string(),
        };
        let local_storage = StorageFactory::create(local_config).unwrap();
        assert_eq!(local_storage.backend_name(), "local");
    }
}
