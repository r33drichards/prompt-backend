//! Local filesystem storage backend implementation

use super::traits::{
    FileMetadata, GetOptions, PutOptions, StorageBackend, StorageError, StorageResult,
};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info, warn};

/// Local filesystem storage backend
///
/// This implementation stores files on the local filesystem in a specified base directory.
/// All paths are relative to the base directory to prevent directory traversal attacks.
///
/// # Example
/// ```no_run
/// use prompt_backend::storage::{LocalStorage, StorageBackend};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let storage = LocalStorage::new("/var/app/storage")?;
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
pub struct LocalStorage {
    base_path: PathBuf,
}

impl LocalStorage {
    /// Create a new local storage backend
    ///
    /// # Arguments
    /// * `base_path` - Base directory for storing files
    ///
    /// # Returns
    /// A new LocalStorage instance, or an error if the directory cannot be created
    pub fn new<P: AsRef<Path>>(base_path: P) -> StorageResult<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        
        // Create base directory if it doesn't exist
        if !base_path.exists() {
            std::fs::create_dir_all(&base_path)?;
            info!("Created storage directory: {}", base_path.display());
        }

        Ok(Self { base_path })
    }

    /// Resolve a relative path to an absolute path within the base directory
    ///
    /// This method prevents directory traversal attacks by ensuring all paths
    /// are contained within the base directory.
    fn resolve_path(&self, path: &str) -> StorageResult<PathBuf> {
        // Remove leading slashes and resolve to prevent traversal
        let clean_path = path.trim_start_matches('/');
        let full_path = self.base_path.join(clean_path);

        // Canonicalize to resolve .. and symlinks, then check it's within base_path
        // Note: We can't use canonicalize on non-existent paths, so we check the parent
        let parent = full_path
            .parent()
            .ok_or_else(|| StorageError::Other("Invalid path".to_string()))?;

        // Create parent directory if it doesn't exist
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }

        // Verify the path is within base_path
        let canonical_base = self
            .base_path
            .canonicalize()
            .map_err(|e| StorageError::Config(format!("Invalid base path: {}", e)))?;

        let canonical_parent = parent.canonicalize().map_err(|e| {
            StorageError::Other(format!("Cannot resolve parent directory: {}", e))
        })?;

        if !canonical_parent.starts_with(&canonical_base) {
            return Err(StorageError::PermissionDenied(
                "Path traversal attempt detected".to_string(),
            ));
        }

        Ok(full_path)
    }

    /// Get file metadata from filesystem
    async fn get_file_metadata(&self, path: &Path) -> StorageResult<FileMetadata> {
        let metadata = fs::metadata(path).await?;
        let path_str = path
            .strip_prefix(&self.base_path)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Try to guess content type from extension
        let content_type = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| mime_guess::from_ext(ext).first())
            .map(|mime| mime.to_string());

        let last_modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        Ok(FileMetadata {
            path: path_str,
            size: metadata.len(),
            content_type,
            last_modified,
            etag: None, // Local storage doesn't generate ETags
        })
    }
}

#[async_trait]
impl StorageBackend for LocalStorage {
    async fn put(
        &self,
        path: &str,
        data: Vec<u8>,
        options: Option<PutOptions>,
    ) -> StorageResult<FileMetadata> {
        let full_path = self.resolve_path(path)?;
        let opts = options.unwrap_or_default();

        // Check if file exists and overwrite is disabled
        if full_path.exists() && !opts.overwrite {
            return Err(StorageError::Other(format!(
                "File already exists: {}",
                path
            )));
        }

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Write file
        let mut file = fs::File::create(&full_path).await?;
        file.write_all(&data).await?;
        file.sync_all().await?;

        debug!("Stored file: {} ({} bytes)", path, data.len());

        self.get_file_metadata(&full_path).await
    }

    async fn get(&self, path: &str, options: Option<GetOptions>) -> StorageResult<Vec<u8>> {
        let full_path = self.resolve_path(path)?;

        if !full_path.exists() {
            return Err(StorageError::NotFound(format!("File not found: {}", path)));
        }

        let mut file = fs::File::open(&full_path).await?;

        // Handle range requests
        if let Some(opts) = options {
            if let Some((start, end)) = opts.range {
                use tokio::io::AsyncSeekExt;
                file.seek(std::io::SeekFrom::Start(start)).await?;
                let length = end - start + 1;
                let mut buffer = vec![0u8; length as usize];
                file.read_exact(&mut buffer).await?;
                debug!("Retrieved file range: {} ({}-{})", path, start, end);
                return Ok(buffer);
            }
        }

        // Read entire file
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;

        debug!("Retrieved file: {} ({} bytes)", path, buffer.len());

        Ok(buffer)
    }

    async fn exists(&self, path: &str) -> StorageResult<bool> {
        let full_path = self.resolve_path(path)?;
        Ok(full_path.exists())
    }

    async fn delete(&self, path: &str) -> StorageResult<()> {
        let full_path = self.resolve_path(path)?;

        if !full_path.exists() {
            return Err(StorageError::NotFound(format!("File not found: {}", path)));
        }

        fs::remove_file(&full_path).await?;

        debug!("Deleted file: {}", path);

        Ok(())
    }

    async fn list(&self, prefix: &str) -> StorageResult<Vec<String>> {
        let prefix_path = self.resolve_path(prefix)?;
        let mut results = Vec::new();

        // If prefix is a file, return it
        if prefix_path.is_file() {
            let relative = prefix_path
                .strip_prefix(&self.base_path)
                .unwrap_or(&prefix_path)
                .to_string_lossy()
                .to_string();
            return Ok(vec![relative]);
        }

        // If prefix doesn't exist, return empty list
        if !prefix_path.exists() {
            return Ok(results);
        }

        // Walk directory tree
        let mut stack = vec![prefix_path];

        while let Some(dir) = stack.pop() {
            let mut entries = fs::read_dir(&dir).await?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();

                if path.is_dir() {
                    stack.push(path);
                } else if path.is_file() {
                    let relative = path
                        .strip_prefix(&self.base_path)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();
                    results.push(relative);
                }
            }
        }

        debug!("Listed {} files with prefix: {}", results.len(), prefix);

        Ok(results)
    }

    async fn metadata(&self, path: &str) -> StorageResult<FileMetadata> {
        let full_path = self.resolve_path(path)?;

        if !full_path.exists() {
            return Err(StorageError::NotFound(format!("File not found: {}", path)));
        }

        self.get_file_metadata(&full_path).await
    }

    async fn get_url(&self, path: &str, _expires_in_secs: Option<u64>) -> StorageResult<String> {
        // Local storage doesn't support public URLs
        // Return file:// URL for local access
        let full_path = self.resolve_path(path)?;

        if !full_path.exists() {
            return Err(StorageError::NotFound(format!("File not found: {}", path)));
        }

        warn!(
            "Local storage does not support public URLs. Returning file:// URL for: {}",
            path
        );

        Ok(format!("file://{}", full_path.display()))
    }

    fn backend_name(&self) -> &str {
        "local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_storage() -> (LocalStorage, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let storage = LocalStorage::new(temp_dir.path()).unwrap();
        (storage, temp_dir)
    }

    #[tokio::test]
    async fn test_put_and_get() {
        let (storage, _temp) = setup_storage().await;
        let data = b"Hello, World!".to_vec();

        // Put file
        let meta = storage.put("test.txt", data.clone(), None).await.unwrap();
        assert_eq!(meta.size, data.len() as u64);

        // Get file
        let retrieved = storage.get("test.txt", None).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_nested_paths() {
        let (storage, _temp) = setup_storage().await;
        let data = b"nested".to_vec();

        // Put in nested path
        storage
            .put("dir1/dir2/file.txt", data.clone(), None)
            .await
            .unwrap();

        // Retrieve
        let retrieved = storage.get("dir1/dir2/file.txt", None).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_exists() {
        let (storage, _temp) = setup_storage().await;

        assert!(!storage.exists("nonexistent.txt").await.unwrap());

        storage
            .put("exists.txt", b"data".to_vec(), None)
            .await
            .unwrap();

        assert!(storage.exists("exists.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let (storage, _temp) = setup_storage().await;

        storage
            .put("delete_me.txt", b"data".to_vec(), None)
            .await
            .unwrap();

        assert!(storage.exists("delete_me.txt").await.unwrap());

        storage.delete("delete_me.txt").await.unwrap();

        assert!(!storage.exists("delete_me.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_list() {
        let (storage, _temp) = setup_storage().await;

        storage.put("a.txt", vec![1], None).await.unwrap();
        storage.put("b.txt", vec![2], None).await.unwrap();
        storage.put("dir/c.txt", vec![3], None).await.unwrap();

        let files = storage.list("").await.unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&"a.txt".to_string()));
        assert!(files.contains(&"b.txt".to_string()));
        assert!(files.contains(&"dir/c.txt".to_string()));
    }

    #[tokio::test]
    async fn test_overwrite_protection() {
        let (storage, _temp) = setup_storage().await;

        storage.put("test.txt", vec![1], None).await.unwrap();

        // Should fail without overwrite flag
        let result = storage.put("test.txt", vec![2], None).await;
        assert!(result.is_err());

        // Should succeed with overwrite flag
        let opts = PutOptions {
            overwrite: true,
            ..Default::default()
        };
        storage.put("test.txt", vec![2], Some(opts)).await.unwrap();

        let data = storage.get("test.txt", None).await.unwrap();
        assert_eq!(data, vec![2]);
    }

    #[tokio::test]
    async fn test_path_traversal_protection() {
        let (storage, _temp) = setup_storage().await;

        // Attempt path traversal
        let result = storage.put("../../../etc/passwd", vec![1], None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_metadata() {
        let (storage, _temp) = setup_storage().await;
        let data = b"Hello".to_vec();

        storage.put("meta.txt", data.clone(), None).await.unwrap();

        let meta = storage.metadata("meta.txt").await.unwrap();
        assert_eq!(meta.size, data.len() as u64);
        assert_eq!(meta.path, "meta.txt");
    }
}
