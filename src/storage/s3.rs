//! Amazon S3 storage backend implementation
//!
//! This module provides S3-compatible storage backend. It supports:
//! - Amazon S3
//! - MinIO
//! - DigitalOcean Spaces
//! - Any S3-compatible storage service

use super::traits::{
    FileMetadata, GetOptions, PutOptions, StorageBackend, StorageError, StorageResult,
};
use async_trait::async_trait;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use aws_sdk_s3::Client;
use tracing::{debug, info};

/// S3 storage backend
///
/// This implementation stores files in Amazon S3 or S3-compatible services.
///
/// # Example
/// ```no_run
/// use prompt_backend::storage::{S3Storage, StorageBackend};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let storage = S3Storage::new(
///         "my-bucket".to_string(),
///         "us-east-1".to_string(),
///         None,
///         None,
///         None,
///     )?;
///     
///     // Store a file
///     storage.put("images/photo.jpg", vec![1, 2, 3], None).await?;
///     
///     // Retrieve it
///     let data = storage.get("images/photo.jpg", None).await?;
///     
///     Ok(())
/// }
/// ```
pub struct S3Storage {
    client: Client,
    bucket: String,
}

impl S3Storage {
    /// Create a new S3 storage backend
    ///
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `region` - AWS region (e.g., "us-east-1")
    /// * `access_key` - Optional AWS access key (uses default credentials if not provided)
    /// * `secret_key` - Optional AWS secret key
    /// * `endpoint` - Optional custom endpoint for S3-compatible services
    ///
    /// # Returns
    /// A new S3Storage instance
    pub fn new(
        bucket: String,
        region: String,
        access_key: Option<String>,
        secret_key: Option<String>,
        endpoint: Option<String>,
    ) -> StorageResult<Self> {
        let client = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                Self::create_client(region, access_key, secret_key, endpoint).await
            })
        })?;

        info!("Created S3 storage backend for bucket: {}", bucket);

        Ok(Self { client, bucket })
    }

    /// Create S3 client with custom configuration
    async fn create_client(
        region: String,
        access_key: Option<String>,
        secret_key: Option<String>,
        endpoint: Option<String>,
    ) -> StorageResult<Client> {
        let region_provider = RegionProviderChain::first_try(region.clone())
            .or_default_provider()
            .or_else(region.as_str());

        let mut config_loader = aws_config::from_env().region(region_provider);

        // Set custom credentials if provided
        if let (Some(key), Some(secret)) = (access_key, secret_key) {
            config_loader = config_loader.credentials_provider(
                aws_sdk_s3::config::Credentials::new(key, secret, None, None, "custom"),
            );
        }

        let config = config_loader.load().await;

        let mut s3_config = aws_sdk_s3::config::Builder::from(&config);

        // Set custom endpoint if provided (for MinIO, DigitalOcean Spaces, etc.)
        if let Some(endpoint_url) = endpoint {
            s3_config = s3_config.endpoint_url(endpoint_url).force_path_style(true);
        }

        Ok(Client::from_conf(s3_config.build()))
    }

    /// Convert S3 SDK error to StorageError
    fn map_s3_error(err: impl std::fmt::Display) -> StorageError {
        let err_str = err.to_string();

        if err_str.contains("NoSuchKey") || err_str.contains("NotFound") {
            StorageError::NotFound(err_str)
        } else if err_str.contains("AccessDenied") || err_str.contains("Forbidden") {
            StorageError::PermissionDenied(err_str)
        } else if err_str.contains("network") || err_str.contains("timeout") {
            StorageError::Network(err_str)
        } else {
            StorageError::Other(err_str)
        }
    }
}

#[async_trait]
impl StorageBackend for S3Storage {
    async fn put(
        &self,
        path: &str,
        data: Vec<u8>,
        options: Option<PutOptions>,
    ) -> StorageResult<FileMetadata> {
        let opts = options.unwrap_or_default();

        // Check if file exists and overwrite is disabled
        if !opts.overwrite {
            if let Ok(true) = self.exists(path).await {
                return Err(StorageError::Other(format!(
                    "File already exists: {}",
                    path
                )));
            }
        }

        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(path)
            .body(ByteStream::from(data.clone()));

        // Set content type if provided
        if let Some(content_type) = opts.content_type {
            request = request.content_type(content_type);
        }

        // Set custom metadata if provided
        if let Some(metadata) = opts.metadata {
            for (key, value) in metadata {
                request = request.metadata(key, value);
            }
        }

        request
            .send()
            .await
            .map_err(|e| Self::map_s3_error(e))?;

        debug!("Stored file in S3: {} ({} bytes)", path, data.len());

        // Get metadata for response
        self.metadata(path).await
    }

    async fn get(&self, path: &str, options: Option<GetOptions>) -> StorageResult<Vec<u8>> {
        let mut request = self.client.get_object().bucket(&self.bucket).key(path);

        // Handle range requests
        if let Some(opts) = options {
            if let Some((start, end)) = opts.range {
                request = request.range(format!("bytes={}-{}", start, end));
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| Self::map_s3_error(e))?;

        let data = response
            .body
            .collect()
            .await
            .map_err(|e| StorageError::Network(e.to_string()))?
            .into_bytes()
            .to_vec();

        debug!("Retrieved file from S3: {} ({} bytes)", path, data.len());

        Ok(data)
    }

    async fn exists(&self, path: &str) -> StorageResult<bool> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("NotFound") || err_str.contains("NoSuchKey") {
                    Ok(false)
                } else {
                    Err(Self::map_s3_error(e))
                }
            }
        }
    }

    async fn delete(&self, path: &str) -> StorageResult<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
            .map_err(|e| Self::map_s3_error(e))?;

        debug!("Deleted file from S3: {}", path);

        Ok(())
    }

    async fn list(&self, prefix: &str) -> StorageResult<Vec<String>> {
        let mut results = Vec::new();
        let mut continuation_token = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);

            if let Some(token) = continuation_token {
                request = request.continuation_token(token);
            }

            let response = request
                .send()
                .await
                .map_err(|e| Self::map_s3_error(e))?;

            if let Some(contents) = response.contents {
                for object in contents {
                    if let Some(key) = object.key {
                        results.push(key);
                    }
                }
            }

            // Check if there are more results
            if response.is_truncated() == Some(true) {
                continuation_token = response.next_continuation_token;
            } else {
                break;
            }
        }

        debug!("Listed {} files with prefix: {}", results.len(), prefix);

        Ok(results)
    }

    async fn metadata(&self, path: &str) -> StorageResult<FileMetadata> {
        let response = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
            .map_err(|e| Self::map_s3_error(e))?;

        let size = response.content_length().unwrap_or(0) as u64;
        let content_type = response.content_type().map(|s| s.to_string());
        let etag = response.e_tag().map(|s| s.to_string());

        let last_modified = response
            .last_modified()
            .and_then(|dt| dt.secs().try_into().ok());

        Ok(FileMetadata {
            path: path.to_string(),
            size,
            content_type,
            last_modified,
            etag,
        })
    }

    async fn get_url(&self, path: &str, expires_in_secs: Option<u64>) -> StorageResult<String> {
        let expires_in = expires_in_secs.unwrap_or(3600); // Default 1 hour

        let presigning_config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
            std::time::Duration::from_secs(expires_in),
        )
        .map_err(|e| StorageError::Other(format!("Invalid expiration time: {}", e)))?;

        let presigned_request = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(path)
            .presigned(presigning_config)
            .await
            .map_err(|e| StorageError::Other(format!("Failed to generate presigned URL: {}", e)))?;

        Ok(presigned_request.uri().to_string())
    }

    fn backend_name(&self) -> &str {
        "s3"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require AWS credentials and an S3 bucket
    // They are disabled by default. To run them:
    // 1. Set AWS credentials in environment
    // 2. Set TEST_S3_BUCKET environment variable
    // 3. Run with: cargo test --features s3_integration_tests

    #[tokio::test]
    #[ignore]
    async fn test_s3_put_and_get() {
        let bucket = std::env::var("TEST_S3_BUCKET").expect("TEST_S3_BUCKET not set");
        let storage = S3Storage::new(bucket, "us-east-1".to_string(), None, None, None).unwrap();

        let data = b"Hello, S3!".to_vec();
        let test_key = format!("test/{}.txt", uuid::Uuid::new_v4());

        // Put file
        storage.put(&test_key, data.clone(), None).await.unwrap();

        // Get file
        let retrieved = storage.get(&test_key, None).await.unwrap();
        assert_eq!(retrieved, data);

        // Cleanup
        storage.delete(&test_key).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_s3_exists() {
        let bucket = std::env::var("TEST_S3_BUCKET").expect("TEST_S3_BUCKET not set");
        let storage = S3Storage::new(bucket, "us-east-1".to_string(), None, None, None).unwrap();

        let test_key = format!("test/{}.txt", uuid::Uuid::new_v4());

        assert!(!storage.exists(&test_key).await.unwrap());

        storage.put(&test_key, b"data".to_vec(), None).await.unwrap();

        assert!(storage.exists(&test_key).await.unwrap());

        storage.delete(&test_key).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_s3_metadata() {
        let bucket = std::env::var("TEST_S3_BUCKET").expect("TEST_S3_BUCKET not set");
        let storage = S3Storage::new(bucket, "us-east-1".to_string(), None, None, None).unwrap();

        let data = b"Hello".to_vec();
        let test_key = format!("test/{}.txt", uuid::Uuid::new_v4());

        storage.put(&test_key, data.clone(), None).await.unwrap();

        let meta = storage.metadata(&test_key).await.unwrap();
        assert_eq!(meta.size, data.len() as u64);

        storage.delete(&test_key).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_s3_presigned_url() {
        let bucket = std::env::var("TEST_S3_BUCKET").expect("TEST_S3_BUCKET not set");
        let storage = S3Storage::new(bucket, "us-east-1".to_string(), None, None, None).unwrap();

        let data = b"Hello".to_vec();
        let test_key = format!("test/{}.txt", uuid::Uuid::new_v4());

        storage.put(&test_key, data.clone(), None).await.unwrap();

        let url = storage.get_url(&test_key, Some(300)).await.unwrap();
        assert!(url.starts_with("https://"));

        storage.delete(&test_key).await.unwrap();
    }
}
