# PostgreSQL vs Filesystem Storage for Image Inputs

## Executive Summary

This document answers two critical questions about image storage for the Claude Code integration:

1. **Can we use filesystem instead of PostgreSQL?** → ✅ **Yes, and it's recommended for production**
2. **What are PostgreSQL JSON blob storage limits?** → **~10 MB recommended, ~255 MB practical maximum**

---

## PostgreSQL JSONB Storage Limits

### Technical Limits

#### Maximum Sizes
- **Hard limit**: ~1 GB per row (TOAST - The Oversized-Attribute Storage Technique)
- **Practical limit**: ~255 MB per JSONB document
- **Recommended limit**: **10 MB** per document for production use
- **Absolute maximum**: 50 MB (beyond this, expect severe performance issues)

#### TOAST (The Oversized-Attribute Storage Technique)
PostgreSQL's TOAST system automatically handles large values:
- Values > 2 KB are compressed first
- Compressed values > ~2 KB are moved to separate TOAST table
- TOAST chunk size: 2000 bytes
- Maximum TOAST value: ~1 GB

#### Performance Impact
Base64 encoding increases image size by ~33%:
- 5 MB JPEG → 6.7 MB base64 → stored in TOAST
- Large JSONB documents negatively affect:
  - **Database backups** (slower, larger)
  - **Replication** (increased lag)
  - **Query performance** (even when not accessing image data)
  - **Memory usage** (during query processing)

### Real-World Image Size Examples

| Image Type | Original | Base64 | PostgreSQL Impact |
|------------|----------|--------|-------------------|
| Screenshot (PNG) | 500 KB | 667 KB | ✅ Excellent |
| Photo (JPEG) | 2 MB | 2.67 MB | ✅ Good |
| High-res screenshot | 5 MB | 6.7 MB | ⚠️ Acceptable |
| Design mockup | 10 MB | 13.3 MB | ⚠️ At limit |
| Multiple images | 20 MB | 26.6 MB | ❌ Not recommended |

**Recommendation**: For MVP, enforce a **10 MB total limit** per prompt (including all images).

---

## Filesystem Storage Alternative

### Architecture

Store image files on disk and keep only metadata references in PostgreSQL.

#### Directory Structure
```
/var/lib/prompt-backend/images/
└── {session_id}/
    └── {prompt_id}/
        ├── image_001.png
        ├── image_002.jpg
        └── metadata.json
```

#### Database Schema (No Migration Required!)
```json
{
  "content": [
    {
      "type": "text",
      "text": "Fix the bug shown in this screenshot"
    },
    {
      "type": "image",
      "source": {
        "type": "file",
        "path": "/var/lib/prompt-backend/images/{session_id}/{prompt_id}/image_001.png",
        "media_type": "image/png",
        "size_bytes": 524288,
        "checksum": "sha256:abc123..."
      }
    }
  ]
}
```

### Comparison Matrix

| Aspect | PostgreSQL JSONB | Filesystem |
|--------|------------------|------------|
| **Max size** | ~255 MB | Unlimited (disk space) |
| **Performance** | Degrades with size | Consistent |
| **DB Backup** | Included (slows backups) | Separate strategy needed |
| **Atomicity** | ✅ Transactional | ⚠️ Requires care |
| **Replication** | Automatic | rsync/NFS/S3 sync |
| **Cleanup** | Automatic (CASCADE) | Manual/scheduled |
| **Access Control** | Database-level | File permissions |
| **Cost** | $0.25/GB (expensive) | $0.005/GB (S3) |
| **Complexity** | Low | Medium |
| **CDN Ready** | No | ✅ Yes |

---

## Implementation: Filesystem Storage

### 1. Configuration

```rust
// src/config.rs
use std::path::PathBuf;

#[derive(Clone)]
pub struct ImageStorageConfig {
    pub base_path: PathBuf,
    pub max_image_size_mb: usize,
    pub retention_days: i64,
}

impl Default for ImageStorageConfig {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from("/var/lib/prompt-backend/images"),
            max_image_size_mb: 10,
            retention_days: 30,
        }
    }
}

pub fn get_image_path(
    config: &ImageStorageConfig,
    session_id: &uuid::Uuid,
    prompt_id: &uuid::Uuid,
    image_index: usize,
    extension: &str,
) -> PathBuf {
    config.base_path
        .join(session_id.to_string())
        .join(prompt_id.to_string())
        .join(format!("image_{:03}.{}", image_index, extension))
}
```

### 2. Image Storage Handler

```rust
// src/handlers/image_storage.rs
use tokio::fs;
use tokio::io::AsyncWriteExt;
use sha2::{Sha256, Digest};

pub async fn save_image_to_filesystem(
    config: &ImageStorageConfig,
    session_id: &uuid::Uuid,
    prompt_id: &uuid::Uuid,
    image_index: usize,
    media_type: &str,
    image_data: &[u8],
) -> Result<PathBuf, std::io::Error> {
    // Validate image size
    let size_mb = image_data.len() / (1024 * 1024);
    if size_mb > config.max_image_size_mb {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Image size {}MB exceeds limit of {}MB", 
                size_mb, config.max_image_size_mb),
        ));
    }

    // Determine file extension
    let extension = match media_type {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Unsupported media type: {}", media_type),
        )),
    };

    // Create directory structure
    let image_path = get_image_path(config, session_id, prompt_id, image_index, extension);
    if let Some(parent) = image_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // Write image file
    let mut file = fs::File::create(&image_path).await?;
    file.write_all(image_data).await?;
    file.sync_all().await?;

    // Calculate SHA256 checksum
    let mut hasher = Sha256::new();
    hasher.update(image_data);
    let checksum = format!("sha256:{:x}", hasher.finalize());

    // Write metadata file
    let metadata = serde_json::json!({
        "media_type": media_type,
        "size_bytes": image_data.len(),
        "checksum": checksum,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "session_id": session_id.to_string(),
        "prompt_id": prompt_id.to_string(),
    });

    let metadata_path = image_path.parent().unwrap().join("metadata.json");
    fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?).await?;

    Ok(image_path)
}

pub async fn read_image_from_filesystem(
    file_path: &str,
) -> Result<Vec<u8>, std::io::Error> {
    tokio::fs::read(file_path).await
}

/// Validate path to prevent directory traversal attacks
pub fn validate_image_path(
    path: &PathBuf,
    base_path: &PathBuf,
) -> Result<(), std::io::Error> {
    let canonical = path.canonicalize()?;
    let base_canonical = base_path.canonicalize()?;

    if !canonical.starts_with(&base_canonical) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Path traversal attempt detected",
        ));
    }

    Ok(())
}
```

### 3. Updated Prompt Handler

```rust
// src/handlers/prompts.rs (updated)
use base64::{Engine as _, engine::general_purpose};

pub async fn create_prompt_with_filesystem_storage(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreatePromptInput>,
) -> Result<Json<CreatePromptResponse>, ApiError> {
    let session_id = input.session_id;
    let prompt_id = uuid::Uuid::new_v4();

    // Process content blocks
    let processed_content = match &input.data["content"] {
        serde_json::Value::Array(blocks) => {
            let mut processed_blocks = Vec::new();
            let mut image_index = 0;
            let mut total_size = 0usize;

            for block in blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        processed_blocks.push(block.clone());
                    }
                    Some("image") => {
                        // Extract base64 image data
                        let base64_data = block["source"]["data"]
                            .as_str()
                            .ok_or(ApiError::BadRequest("Missing image data".into()))?;
                        let media_type = block["source"]["media_type"]
                            .as_str()
                            .ok_or(ApiError::BadRequest("Missing media type".into()))?;

                        // Decode base64
                        let image_data = general_purpose::STANDARD
                            .decode(base64_data)
                            .map_err(|e| ApiError::BadRequest(format!("Invalid base64: {}", e)))?;

                        total_size += image_data.len();

                        // Enforce total size limit (10 MB)
                        if total_size > 10 * 1024 * 1024 {
                            return Err(ApiError::BadRequest(
                                "Total image size exceeds 10 MB limit".into()
                            ));
                        }

                        // Save to filesystem
                        let image_path = save_image_to_filesystem(
                            &state.image_storage_config,
                            &session_id,
                            &prompt_id,
                            image_index,
                            media_type,
                            &image_data,
                        ).await
                        .map_err(|e| ApiError::InternalError(format!("Failed to save image: {}", e)))?;

                        // Create file reference (store in DB)
                        let file_block = serde_json::json!({
                            "type": "image",
                            "source": {
                                "type": "file",
                                "path": image_path.to_string_lossy(),
                                "media_type": media_type,
                                "size_bytes": image_data.len(),
                            }
                        });

                        processed_blocks.push(file_block);
                        image_index += 1;
                    }
                    _ => {
                        return Err(ApiError::BadRequest("Unknown content block type".into()));
                    }
                }
            }

            serde_json::json!({ "content": processed_blocks })
        }
        _ => input.data, // Legacy text-only format
    };

    // Save to database
    let new_prompt = prompt::ActiveModel {
        id: Set(prompt_id),
        session_id: Set(session_id),
        data: Set(processed_content),
        created_at: NotSet,
        updated_at: NotSet,
    };

    new_prompt.insert(&state.db).await
        .map_err(|e| ApiError::InternalError(format!("Database error: {}", e)))?;

    Ok(Json(CreatePromptResponse { id: prompt_id }))
}
```

### 4. Updated Outbox Publisher

```rust
// src/workers/outbox_publisher.rs (updated)

async fn extract_prompt_content_with_filesystem(
    prompt_model: &prompt::Model,
    storage_config: &ImageStorageConfig,
) -> Result<String, Error> {
    match &prompt_model.data["content"] {
        serde_json::Value::Array(blocks) => {
            let mut processed_blocks = Vec::new();

            for block in blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        processed_blocks.push(block.clone());
                    }
                    Some("image") if block["source"]["type"] == "file" => {
                        // Read image from filesystem
                        let file_path = block["source"]["path"]
                            .as_str()
                            .ok_or("Missing file path")?;

                        let image_data = tokio::fs::read(file_path).await
                            .map_err(|e| {
                                error!("Failed to read image from {}: {}", file_path, e);
                                Error::Failed(Box::new(e))
                            })?;

                        // Convert to base64 for Claude API
                        let base64_data = general_purpose::STANDARD.encode(&image_data);

                        // Create Claude API format
                        let image_block = serde_json::json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": block["source"]["media_type"],
                                "data": base64_data,
                            }
                        });

                        processed_blocks.push(image_block);

                        info!("Loaded image from filesystem: {} ({} bytes)", 
                            file_path, image_data.len());
                    }
                    Some("image") if block["source"]["type"] == "base64" => {
                        // Already in base64 format (backward compatibility)
                        processed_blocks.push(block.clone());
                    }
                    _ => {
                        warn!("Unknown content block type: {:?}", block["type"]);
                    }
                }
            }

            Ok(serde_json::to_string(&serde_json::json!({
                "content": processed_blocks
            }))?)
        }
        _ => {
            // Legacy text-only format
            Ok(serde_json::to_string(&prompt_model.data)?)
        }
    }
}
```

### 5. Cleanup Job

```rust
// src/workers/image_cleanup.rs (new file)
use apalis::prelude::*;
use tokio::fs;
use chrono::{DateTime, Utc, Duration};
use tracing::{info, error, warn};

/// Scheduled job to clean up old image files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageCleanupJob {
    pub retention_days: i64,
}

impl Job for ImageCleanupJob {
    const NAME: &'static str = "ImageCleanupJob";
}

pub async fn cleanup_old_images(
    job: ImageCleanupJob,
    ctx: Data<ImageStorageConfig>,
) -> Result<(), Error> {
    info!("Starting image cleanup job (retention: {} days)", job.retention_days);

    let cutoff_date = Utc::now() - Duration::days(job.retention_days);
    let mut deleted_count = 0;
    let mut total_size_freed = 0u64;

    let mut entries = fs::read_dir(&ctx.base_path).await
        .map_err(|e| Error::Failed(Box::new(e)))?;

    while let Some(entry) = entries.next_entry().await
        .map_err(|e| Error::Failed(Box::new(e)))? {

        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Check metadata.json for creation date
        let metadata_path = path.join("metadata.json");
        if !metadata_path.exists() {
            warn!("No metadata.json found in {:?}, skipping", path);
            continue;
        }

        let metadata_str = fs::read_to_string(&metadata_path).await
            .map_err(|e| Error::Failed(Box::new(e)))?;

        let metadata: serde_json::Value = serde_json::from_str(&metadata_str)
            .map_err(|e| Error::Failed(Box::new(e)))?;

        if let Some(created_at) = metadata["created_at"].as_str() {
            let created = DateTime::parse_from_rfc3339(created_at)
                .map_err(|e| Error::Failed(Box::new(e)))?;

            if created.with_timezone(&Utc) < cutoff_date {
                // Calculate size before deletion
                if let Ok(dir_size) = get_dir_size(&path).await {
                    total_size_freed += dir_size;
                }

                // Delete entire directory
                fs::remove_dir_all(&path).await
                    .map_err(|e| Error::Failed(Box::new(e)))?;

                deleted_count += 1;
                info!("Deleted old images from {:?}", path);
            }
        }
    }

    info!(
        "Cleanup complete: deleted {} directories, freed {} MB",
        deleted_count,
        total_size_freed / (1024 * 1024)
    );

    Ok(())
}

async fn get_dir_size(path: &std::path::Path) -> Result<u64, std::io::Error> {
    let mut total = 0u64;
    let mut entries = fs::read_dir(path).await?;

    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        total += metadata.len();
    }

    Ok(total)
}
```

---

## Security Considerations

### File System Security

1. **Directory Permissions**
```bash
# Set restrictive permissions
sudo mkdir -p /var/lib/prompt-backend/images
sudo chown prompt-backend:prompt-backend /var/lib/prompt-backend/images
sudo chmod 750 /var/lib/prompt-backend/images
```

2. **Path Traversal Protection** (implemented in `validate_image_path`)
3. **File Type Validation** (check magic bytes, not just extension)
4. **Size Limits** (enforced at upload time)
5. **Optional: Virus Scanning**

```rust
async fn scan_image_for_viruses(image_path: &PathBuf) -> Result<bool, std::io::Error> {
    let output = tokio::process::Command::new("clamdscan")
        .arg("--no-summary")
        .arg(image_path)
        .output()
        .await?;

    Ok(output.status.success())
}
```

---

## Cost Analysis

### Storage Costs Comparison

| Solution | Cost/GB/Month | 100 GB Cost | Notes |
|----------|---------------|-------------|-------|
| **PostgreSQL** | $0.25 | $25 | Affects backup/replication |
| **Railway Volume** | $0.25 | $25 | Better performance |
| **AWS S3 Standard** | $0.023 | $2.30 | + bandwidth costs |
| **Backblaze B2** | $0.005 | $0.50 | Best value |

### Bandwidth Costs (if serving images directly)
- Backblaze B2: First 3× storage is free, then $0.01/GB
- AWS S3: $0.09/GB (use CloudFront CDN instead)
- Railway: Included in plan

**Recommendation**: 
- MVP: Local filesystem on Railway volume
- Scale: Migrate to Backblaze B2 when storage > 10 GB

---

## Migration Path

### Phase 1: MVP (Now) - PostgreSQL JSONB ✅
- **Current state**: Store base64 in JSONB
- **Limit**: 10 MB total per prompt
- **Timeline**: Complete (existing design)
- **Benefits**: Simple, fast to ship

### Phase 2: Filesystem Storage (Next)
- **Implementation**: Use local filesystem
- **Backward compatible**: Support both formats
- **Timeline**: 2-3 weeks
- **Benefits**: Better performance, no DB bloat

### Phase 3: Object Storage (Future)
- **Implementation**: Migrate to S3/B2
- **Features**: CDN integration, presigned URLs
- **Timeline**: When storage > 10 GB
- **Benefits**: Scalability, lower cost

---

## Recommendation

### For Current MVP: Keep PostgreSQL
✅ **Use PostgreSQL JSONB with strict limits**
- Simple implementation (already designed)
- 10 MB limit covers 90% of use cases
- Fast to ship and validate
- Clear upgrade path documented

### For Next Iteration: Filesystem
✅ **Migrate to filesystem storage**
- Implement within 2-3 weeks
- Better performance and scalability
- Maintain backward compatibility
- Prepare for object storage migration

### Decision Matrix

Use **PostgreSQL JSONB** if:
- Images are typically < 2 MB
- Low volume (< 100 prompts/day)
- Need simple atomicity guarantees
- Want fastest MVP iteration

Use **Filesystem** if:
- Images are typically > 2 MB
- Higher volume (> 100 prompts/day)
- Need better performance
- Planning to scale beyond MVP

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_save_and_read_image() {
        let config = ImageStorageConfig::default();
        let session_id = uuid::Uuid::new_v4();
        let prompt_id = uuid::Uuid::new_v4();

        let test_image = vec![0xFF, 0xD8, 0xFF]; // Minimal JPEG header

        let path = save_image_to_filesystem(
            &config,
            &session_id,
            &prompt_id,
            0,
            "image/jpeg",
            &test_image,
        ).await.unwrap();

        let read_image = read_image_from_filesystem(path.to_str().unwrap())
            .await.unwrap();

        assert_eq!(test_image, read_image);
    }

    #[tokio::test]
    async fn test_image_size_limit() {
        let config = ImageStorageConfig {
            max_image_size_mb: 1,
            ..Default::default()
        };

        let large_image = vec![0u8; 2 * 1024 * 1024]; // 2 MB

        let result = save_image_to_filesystem(
            &config,
            &uuid::Uuid::new_v4(),
            &uuid::Uuid::new_v4(),
            0,
            "image/jpeg",
            &large_image,
        ).await;

        assert!(result.is_err());
    }
}
```

---

## Deployment Checklist

### Filesystem Storage Deployment

- [ ] Create image storage directory
- [ ] Set proper permissions
- [ ] Configure environment variable `IMAGE_STORAGE_PATH`
- [ ] Deploy updated API handlers
- [ ] Deploy updated outbox worker
- [ ] Set up cleanup job (cron)
- [ ] Monitor disk usage
- [ ] Test backup/restore process
- [ ] Document for team

### Environment Variables

```bash
# .env
IMAGE_STORAGE_PATH=/var/lib/prompt-backend/images
IMAGE_MAX_SIZE_MB=10
IMAGE_RETENTION_DAYS=30
```

---

## Monitoring

### Metrics to Track

```rust
// Prometheus metrics
lazy_static! {
    static ref IMAGE_UPLOADS: IntCounter = register_int_counter!(
        "image_uploads_total",
        "Total number of image uploads"
    ).unwrap();

    static ref IMAGE_SIZE_BYTES: Histogram = register_histogram!(
        "image_size_bytes",
        "Size of uploaded images in bytes"
    ).unwrap();

    static ref IMAGE_STORAGE_DISK_USAGE: Gauge = register_gauge!(
        "image_storage_disk_usage_bytes",
        "Current disk usage for image storage"
    ).unwrap();
}
```

### Alerts

1. **Disk Usage > 80%**: Scale up or clean old files
2. **Image Upload Failures > 5%**: Check filesystem permissions
3. **Image Read Errors**: Missing files or corruption

---

## Appendix: Quick Reference

### PostgreSQL JSONB Limits
- **Recommended**: 10 MB
- **Practical max**: 255 MB
- **Hard limit**: 1 GB (TOAST)

### When to Use What
- **< 2 MB images**: PostgreSQL is fine
- **2-10 MB images**: PostgreSQL okay, filesystem better
- **> 10 MB images**: Filesystem required

### File Paths
- **Storage**: `/var/lib/prompt-backend/images/{session_id}/{prompt_id}/`
- **Config**: `IMAGE_STORAGE_PATH` env variable

### Cost Estimate (100 prompts/day, 3 MB avg)
- **PostgreSQL**: $2.25/month (9 GB × $0.25)
- **Filesystem**: $2.25/month (same Railway volume)
- **Backblaze B2**: $0.045/month (9 GB × $0.005) ⭐ Best value at scale
