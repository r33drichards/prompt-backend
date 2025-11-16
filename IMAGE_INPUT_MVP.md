# Image Input MVP for Claude Code Integration

## Overview

This document outlines an MVP (Minimum Viable Product) approach to accepting image inputs in prompts for Claude Code processing. The solution enables users to attach images to their prompts, which Claude Code can then analyze alongside text instructions.

## Current Architecture Context

The system currently handles text-only prompts through:
1. REST API endpoint (`POST /prompts`) that accepts JSON data
2. Prompt storage in PostgreSQL with a JSONB `data` field
3. Background job processing via `outbox_publisher.rs` that:
   - Extracts text content from `prompt.data`
   - Sets up a sandbox environment
   - Passes content to Claude Code CLI
   - Streams responses back as messages

## MVP Approach: Base64-Encoded Images in JSON

### Design Rationale

**Why Base64 in JSON (for MVP)?**
- ✅ No additional infrastructure needed (no object storage, CDN, or file server)
- ✅ Works with existing JSONB column in PostgreSQL
- ✅ Minimal code changes to existing handlers
- ✅ Transactional consistency (images stored with prompt atomically)
- ✅ Simple client implementation
- ✅ Claude API already supports base64 image format
- ⚠️ Database size consideration (addressed with limits)

**Future enhancements** (post-MVP) could include object storage (S3) with presigned URLs if needed.

## Technical Specification

### 1. Data Model Changes

**No schema migration required!** The existing `prompt.data` JSONB column can accommodate the new structure.

#### Current Prompt Data Format
```json
{
  "content": "Fix the bug in auth.rs"
}
```

#### New Format with Images
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
        "type": "base64",
        "media_type": "image/png",
        "data": "iVBORw0KGgoAAAANSUhEUgA..."
      }
    }
  ]
}
```

This follows Anthropic's [message content format](https://docs.anthropic.com/en/api/messages) for vision capabilities.

### 2. API Changes

#### Updated `CreatePromptInput` Structure

**File**: `src/handlers/prompts.rs`

```rust
#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Image { source: ImageSource },
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String, // "base64"
    pub media_type: String,  // "image/jpeg", "image/png", "image/gif", "image/webp"
    pub data: String,        // base64-encoded image data
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(untagged)]
pub enum PromptContent {
    // Legacy format: plain string
    Text(String),
    // Legacy format: object with "content" field
    LegacyObject { content: String },
    // New format: array of content blocks (text + images)
    Blocks(Vec<ContentBlock>),
}

// The CreatePromptInput remains flexible
#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreatePromptInput {
    pub session_id: String,
    pub data: serde_json::Value, // Still accepts any JSON
}
```

#### Validation Logic

Add validation to the `create` handler:

```rust
// In src/handlers/prompts.rs::create()
fn validate_prompt_data(data: &serde_json::Value) -> Result<(), Error> {
    // Check total size (10MB limit for MVP)
    let json_size = serde_json::to_string(data)
        .map_err(|e| Error::bad_request(format!("Invalid JSON: {}", e)))?
        .len();
    
    if json_size > 10 * 1024 * 1024 {
        return Err(Error::bad_request(
            "Prompt data exceeds 10MB limit".to_string()
        ));
    }

    // If data contains content array, validate images
    if let Some(content) = data.get("content") {
        if let Some(blocks) = content.as_array() {
            for block in blocks {
                if block.get("type") == Some(&json!("image")) {
                    validate_image_block(block)?;
                }
            }
        }
    }

    Ok(())
}

fn validate_image_block(block: &serde_json::Value) -> Result<(), Error> {
    let source = block.get("source")
        .ok_or_else(|| Error::bad_request("Image block missing 'source'".to_string()))?;
    
    let media_type = source.get("media_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::bad_request("Image source missing 'media_type'".to_string()))?;
    
    // Validate supported formats
    const SUPPORTED_FORMATS: &[&str] = &["image/jpeg", "image/png", "image/gif", "image/webp"];
    if !SUPPORTED_FORMATS.contains(&media_type) {
        return Err(Error::bad_request(format!(
            "Unsupported image format: {}. Supported: {:?}",
            media_type, SUPPORTED_FORMATS
        )));
    }

    // Validate base64 data exists and is valid
    let data_str = source.get("data")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::bad_request("Image source missing 'data'".to_string()))?;
    
    // Quick validation: check if it's valid base64
    if base64::decode(data_str).is_err() {
        return Err(Error::bad_request("Invalid base64 image data".to_string()));
    }

    Ok(())
}
```

### 3. Background Job Processing Changes

**File**: `src/bg_tasks/outbox_publisher.rs`

Update the prompt content extraction logic to handle the new format:

```rust
// Extract prompt content from the data field
let prompt_content = extract_prompt_content(&prompt_model.data)?;

fn extract_prompt_content(data: &serde_json::Value) -> Result<String, Error> {
    match data {
        // Legacy: plain string
        serde_json::Value::String(s) => Ok(s.clone()),
        
        // Object format
        serde_json::Value::Object(obj) => {
            if let Some(content_value) = obj.get("content") {
                match content_value {
                    // Legacy: string content
                    serde_json::Value::String(s) => Ok(s.clone()),
                    
                    // New: array of content blocks (may include images)
                    serde_json::Value::Array(blocks) => {
                        format_content_blocks(blocks)
                    }
                    
                    _ => Ok(serde_json::to_string(data).unwrap_or_default())
                }
            } else {
                // Try legacy field names
                obj.get("prompt")
                    .or_else(|| obj.get("text"))
                    .or_else(|| obj.get("message"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| Error::Failed("Could not extract prompt content".into()))
            }
        }
        
        _ => Ok(serde_json::to_string(data).unwrap_or_default()),
    }
}

fn format_content_blocks(blocks: &[serde_json::Value]) -> Result<String, Error> {
    let mut result = String::new();
    let mut image_count = 0;

    for block in blocks {
        let block_type = block.get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("text");

        match block_type {
            "text" => {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    result.push_str(text);
                    result.push('\n');
                }
            }
            "image" => {
                image_count += 1;
                // For Claude Code CLI, we'll need to save images to temp files
                // and reference them in the prompt
                result.push_str(&format!("[Image {}]\n", image_count));
                // Note: We'll handle the actual image processing below
            }
            _ => {
                // Unknown block type, skip
                continue;
            }
        }
    }

    Ok(result)
}
```

### 4. Claude Code CLI Integration

Claude Code CLI supports image inputs via the API. We need to:

1. **Save images to temporary files** in the session directory
2. **Pass image paths or base64 data** to Claude Code CLI

**Updated approach in `process_outbox_job`**:

```rust
// After extracting prompt content, check if there are images
let images = extract_images(&prompt_model.data)?;

if !images.is_empty() {
    info!("Prompt contains {} images", images.len());
    
    // Save images to temporary files in the session directory
    for (idx, image) in images.iter().enumerate() {
        let image_filename = format!("prompt_image_{}.{}", idx + 1, image.extension());
        let image_path = temp_dir.path().join(&image_filename);
        
        // Decode base64 and write to file
        let image_data = base64::decode(&image.data)
            .map_err(|e| {
                error!("Failed to decode base64 image: {}", e);
                Error::Failed(Box::new(e))
            })?;
        
        std::fs::write(&image_path, &image_data)
            .map_err(|e| {
                error!("Failed to write image file: {}", e);
                Error::Failed(Box::new(e))
            })?;
        
        info!("Saved image to: {}", image_path.display());
    }
}

// Extract images from prompt data
fn extract_images(data: &serde_json::Value) -> Result<Vec<ImageData>, Error> {
    let mut images = Vec::new();

    if let Some(content) = data.get("content") {
        if let Some(blocks) = content.as_array() {
            for block in blocks {
                if block.get("type") == Some(&json!("image")) {
                    if let Some(source) = block.get("source") {
                        let media_type = source.get("media_type")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| Error::Failed("Missing media_type".into()))?;
                        
                        let data = source.get("data")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| Error::Failed("Missing image data".into()))?;
                        
                        images.push(ImageData {
                            media_type: media_type.to_string(),
                            data: data.to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(images)
}

struct ImageData {
    media_type: String,
    data: String,
}

impl ImageData {
    fn extension(&self) -> &str {
        match self.media_type.as_str() {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "bin",
        }
    }
}
```

**For Claude Code CLI**, the images would be referenced in the prompt. Since Claude Code uses the Messages API format internally, we would construct the proper format when invoking Claude:

```rust
// When preparing the Claude Code invocation, format the prompt appropriately
let formatted_prompt_for_claude = if !images.is_empty() {
    // Convert to Anthropic Messages API format
    let content_blocks = construct_content_blocks_with_images(&prompt_content, &images);
    serde_json::to_string(&content_blocks).unwrap()
} else {
    prompt_content
};
```

**Note**: The current implementation uses `claude` CLI with `-p` flag. We may need to:
- Use a temporary JSON file with the full message structure
- Pass it via `--message-file` or similar flag
- Or use the API directly instead of CLI for image support

### 5. Alternative: Direct API Integration

For more control, we could bypass the Claude Code CLI for image-containing prompts and use the Anthropic API directly:

```rust
// Check if prompt contains images
if extract_images(&prompt_model.data)?.is_empty() {
    // Use existing CLI approach for text-only
    spawn_claude_cli_process(...);
} else {
    // Use API directly for image-containing prompts
    spawn_claude_api_interaction(...).await?;
}

async fn spawn_claude_api_interaction(
    prompt_data: &serde_json::Value,
    system_prompt: &str,
    // ... other params
) -> Result<(), Error> {
    let anthropic_api_key = std::env::var("ANTHROPIC_API_KEY")?;
    let client = reqwest::Client::new();
    
    // Construct messages in Anthropic format
    let messages = vec![json!({
        "role": "user",
        "content": prompt_data.get("content").unwrap_or(&json!(""))
    })];
    
    let request_body = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 4096,
        "system": system_prompt,
        "messages": messages
    });
    
    // Stream the response
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", anthropic_api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&request_body)
        .send()
        .await?;
    
    // Process streaming response and save to messages table
    // ... (similar to CLI output processing)
}
```

## MVP Implementation Plan

### Phase 1: API Layer (Week 1)
1. ✅ Update `CreatePromptInput` types to document the new format
2. ✅ Add validation functions for image data
3. ✅ Add size limits (10MB total per prompt)
4. ✅ Update OpenAPI documentation
5. ✅ Add unit tests for validation

### Phase 2: Storage & Processing (Week 2)
1. ✅ Update `extract_prompt_content` to handle new format
2. ✅ Add `extract_images` function
3. ✅ Implement image file saving in temp directory
4. ✅ Add integration tests

### Phase 3: Claude Integration (Week 3)
1. ✅ Research Claude Code CLI image support
2. ✅ Implement either:
   - Option A: CLI with image file references
   - Option B: Direct API integration
3. ✅ Test end-to-end with sample images

### Phase 4: Testing & Documentation (Week 4)
1. ✅ End-to-end integration tests
2. ✅ Performance testing (latency with images)
3. ✅ Update API documentation with examples
4. ✅ Create client SDK examples

## Example Usage

### Client Request

```bash
curl -X POST https://api.example.com/prompts \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "session_id": "550e8400-e29b-41d4-a716-446655440000",
    "data": {
      "content": [
        {
          "type": "text",
          "text": "Analyze this error screenshot and suggest a fix"
        },
        {
          "type": "image",
          "source": {
            "type": "base64",
            "media_type": "image/png",
            "data": "iVBORw0KGgoAAAANSUhEUg..."
          }
        }
      ]
    }
  }'
```

### TypeScript Client Example

```typescript
import { Client } from '@wholelottahoopla/prompt-backend-client';
import fs from 'fs';

const client = new Client({ token: process.env.AUTH_TOKEN });

// Read image file
const imageBuffer = fs.readFileSync('error_screenshot.png');
const imageBase64 = imageBuffer.toString('base64');

// Create prompt with image
const result = await client.prompts.create({
  session_id: sessionId,
  data: {
    content: [
      {
        type: 'text',
        text: 'Fix the bug shown in this screenshot'
      },
      {
        type: 'image',
        source: {
          type: 'base64',
          media_type: 'image/png',
          data: imageBase64
        }
      }
    ]
  }
});
```

## Constraints & Limitations (MVP)

1. **Size Limit**: 10MB total per prompt (including all images)
2. **Supported Formats**: JPEG, PNG, GIF, WebP
3. **Image Count**: Recommended max 5 images per prompt (API limit: 20)
4. **No Image Processing**: No resizing, compression, or optimization
5. **Database Storage**: Images stored in PostgreSQL (not object storage)
6. **No Image CDN**: No public URLs for images

## Future Enhancements (Post-MVP)

1. **Object Storage Integration**
   - Store images in S3/GCS
   - Generate presigned URLs
   - Reduce database size

2. **Image Optimization**
   - Automatic compression
   - Format conversion
   - Thumbnail generation

3. **Advanced Features**
   - Image annotation support
   - OCR text extraction
   - Multi-image comparison

4. **Performance Optimizations**
   - Image caching
   - CDN integration
   - Lazy loading

## Testing Strategy

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_image_data_valid() {
        let data = json!({
            "content": [{
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "iVBORw0KGg=="
                }
            }]
        });
        
        assert!(validate_prompt_data(&data).is_ok());
    }

    #[test]
    fn test_validate_image_data_invalid_format() {
        let data = json!({
            "content": [{
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/bmp",
                    "data": "..."
                }
            }]
        });
        
        assert!(validate_prompt_data(&data).is_err());
    }

    #[test]
    fn test_extract_images() {
        let data = json!({
            "content": [
                {"type": "text", "text": "hello"},
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "abc123"
                    }
                }
            ]
        });
        
        let images = extract_images(&data).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].media_type, "image/png");
    }
}
```

### Integration Tests
```rust
#[tokio::test]
async fn test_create_prompt_with_image() {
    let (client, db) = setup_test_environment().await;
    
    let image_data = include_bytes!("../fixtures/test_image.png");
    let image_base64 = base64::encode(image_data);
    
    let response = client
        .post("/prompts")
        .json(&json!({
            "session_id": test_session_id,
            "data": {
                "content": [
                    {"type": "text", "text": "Analyze this image"},
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": image_base64
                        }
                    }
                ]
            }
        }))
        .send()
        .await;
    
    assert_eq!(response.status(), 200);
}
```

## Security Considerations

1. **Input Validation**
   - Validate base64 encoding
   - Check file size limits
   - Verify MIME types
   - Sanitize all user inputs

2. **Rate Limiting**
   - Implement stricter rate limits for image uploads
   - Prevent abuse and DoS attacks

3. **Access Control**
   - Ensure users can only access their own prompts/images
   - Implement proper authentication checks

4. **Data Privacy**
   - Images are stored with same security as prompts
   - Consider encryption at rest for sensitive images
   - Implement data retention policies

## Monitoring & Observability

Add metrics for:
- Number of prompts with images
- Average image size
- Image processing time
- Validation failures
- Storage usage

```rust
// Example metrics
counter!("prompts.created.with_images", 1);
histogram!("prompts.image_size_bytes", image_size as f64);
histogram!("prompts.image_processing_duration_ms", duration.as_millis() as f64);
```

## Rollback Plan

If issues arise:
1. API remains backward compatible (text-only prompts work)
2. Feature flag to disable image processing
3. Validation errors prevent corrupt data
4. Database schema unchanged (easy rollback)

## Success Metrics

- ✅ Users can successfully submit prompts with images
- ✅ Claude Code processes images alongside text
- ✅ Response latency < 5s additional overhead for images
- ✅ Zero data corruption or loss
- ✅ API success rate > 99.9%

## Conclusion

This MVP provides a straightforward path to image support by:
- Leveraging existing JSONB storage (no migration needed)
- Following Anthropic's standard content format
- Minimizing infrastructure changes
- Maintaining backward compatibility

The approach can be implemented incrementally and scaled to object storage later if needed.
