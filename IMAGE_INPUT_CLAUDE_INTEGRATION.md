# Image Input MVP: Claude Code Integration Strategy

## Executive Summary

This document provides a refined MVP approach for accepting image inputs in prompts, focusing specifically on the integration with Claude Code and the MCP (Model Context Protocol) sandbox architecture currently used in the system.

## Problem Statement

The current implementation:
1. Accepts only text prompts via JSON API
2. Processes prompts through Claude Code CLI in a sandboxed environment
3. Uses MCP for sandbox communication
4. Cannot handle visual inputs like screenshots, diagrams, or error images

## Key Insight: Claude API Direct Integration

After analyzing the codebase, the **critical realization** is that Claude Code CLI may not directly support image inputs in the way we need. Therefore, the MVP should:

1. **Continue using Claude Code CLI for text-only prompts** (existing behavior)
2. **Use Claude API directly for prompts with images** (new behavior)
3. **Maintain the same output format** (streaming JSON messages)

This hybrid approach provides:
- ✅ Minimal disruption to existing text-based workflows
- ✅ Full Claude vision capabilities for image inputs
- ✅ Consistent output format and database schema
- ✅ Easy A/B testing and rollback

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                   Client Request                         │
│            POST /prompts with JSON payload               │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│              Rocket API Handler                          │
│   - Validates input (size, format, mime type)            │
│   - Detects if prompt contains images                    │
│   - Stores in PostgreSQL (JSONB column)                  │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│           Background Job (outbox_publisher)              │
│   - Extracts prompt from database                        │
│   - Checks for images in prompt.data                     │
└───────────────────────┬─────────────────────────────────┘
                        │
            ┌───────────┴────────────┐
            │                        │
            ▼                        ▼
    ┌──────────────┐        ┌──────────────────┐
    │ Text Only?   │        │ Contains Images? │
    │ Use Claude   │        │ Use Claude API   │
    │ Code CLI     │        │ Directly         │
    │ (existing)   │        │ (new)            │
    └──────┬───────┘        └────────┬─────────┘
           │                         │
           │                         ▼
           │                ┌─────────────────────┐
           │                │ Save images to temp │
           │                │ files, construct    │
           │                │ Messages API request│
           │                └──────────┬──────────┘
           │                           │
           │                           ▼
           │                  ┌─────────────────┐
           │                  │ Stream response │
           │                  │ from Claude API │
           │                  └────────┬────────┘
           │                           │
           └───────────┬───────────────┘
                       │
                       ▼
        ┌──────────────────────────────┐
        │  Parse and save messages     │
        │  to PostgreSQL (same format) │
        └──────────────────────────────┘
```

## Implementation Plan

### Phase 1: Input Validation (Week 1)

**File**: `src/handlers/prompts.rs`

Add validation logic to the `create` handler:

```rust
use base64::{Engine as _, engine::general_purpose};

/// Create a new prompt
#[openapi]
#[post("/prompts", data = "<input>")]
pub async fn create(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    input: Json<CreatePromptInput>,
) -> OResult<CreatePromptOutput> {
    // ... existing session verification code ...

    // Validate the prompt data
    validate_prompt_data(&input.data)?;

    // ... rest of existing code ...
}

/// Validate prompt data including image blocks
fn validate_prompt_data(data: &serde_json::Value) -> Result<(), Error> {
    // Check total size (10MB limit for MVP)
    let json_str = serde_json::to_string(data)
        .map_err(|e| Error::bad_request(format!("Invalid JSON: {}", e)))?;
    
    let size_bytes = json_str.len();
    const MAX_SIZE: usize = 10 * 1024 * 1024; // 10MB
    
    if size_bytes > MAX_SIZE {
        return Err(Error::bad_request(format!(
            "Prompt data exceeds {}MB limit (got {}MB)",
            MAX_SIZE / 1024 / 1024,
            size_bytes / 1024 / 1024
        )));
    }

    // If data contains content array, validate images
    if let Some(content) = data.get("content") {
        if let Some(blocks) = content.as_array() {
            let mut image_count = 0;
            
            for (idx, block) in blocks.iter().enumerate() {
                if let Some(block_type) = block.get("type").and_then(|v| v.as_str()) {
                    if block_type == "image" {
                        image_count += 1;
                        validate_image_block(block, idx)?;
                    }
                }
            }
            
            // Claude API supports up to 20 images, but we limit to 5 for MVP
            if image_count > 5 {
                return Err(Error::bad_request(format!(
                    "Too many images (max 5, got {})",
                    image_count
                )));
            }
            
            if image_count > 0 {
                tracing::info!("Prompt contains {} images", image_count);
            }
        }
    }

    Ok(())
}

/// Validate a single image block
fn validate_image_block(block: &serde_json::Value, idx: usize) -> Result<(), Error> {
    let source = block.get("source")
        .ok_or_else(|| Error::bad_request(
            format!("Image block {} missing 'source' field", idx)
        ))?;
    
    // Validate source type
    let source_type = source.get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::bad_request(
            format!("Image block {} missing 'type' in source", idx)
        ))?;
    
    if source_type != "base64" {
        return Err(Error::bad_request(format!(
            "Image block {} has unsupported source type '{}' (only 'base64' supported)",
            idx, source_type
        )));
    }
    
    // Validate media type
    let media_type = source.get("media_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::bad_request(
            format!("Image block {} missing 'media_type'", idx)
        ))?;
    
    const SUPPORTED_TYPES: &[&str] = &["image/jpeg", "image/png", "image/gif", "image/webp"];
    if !SUPPORTED_TYPES.contains(&media_type) {
        return Err(Error::bad_request(format!(
            "Image block {} has unsupported media type '{}' (supported: {:?})",
            idx, media_type, SUPPORTED_TYPES
        )));
    }
    
    // Validate base64 data
    let data_str = source.get("data")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::bad_request(
            format!("Image block {} missing 'data' field", idx)
        ))?;
    
    // Validate base64 encoding
    general_purpose::STANDARD
        .decode(data_str)
        .map_err(|e| Error::bad_request(format!(
            "Image block {} has invalid base64 data: {}",
            idx, e
        )))?;
    
    // Check individual image size (max 5MB per image)
    let decoded_size = data_str.len() * 3 / 4; // Approximate decoded size
    const MAX_IMAGE_SIZE: usize = 5 * 1024 * 1024; // 5MB
    
    if decoded_size > MAX_IMAGE_SIZE {
        return Err(Error::bad_request(format!(
            "Image block {} exceeds 5MB limit (got ~{}MB)",
            idx, decoded_size / 1024 / 1024
        )));
    }

    Ok(())
}
```

**Add to `Cargo.toml`**:
```toml
base64 = "0.21"
```

### Phase 2: Background Processing Logic (Week 2)

**File**: `src/bg_tasks/outbox_publisher.rs`

Update the background job to detect and handle images:

```rust
// In process_outbox_job function, after extracting prompt_content:

// Check if prompt contains images
let has_images = contains_images(&prompt_model.data);

if has_images {
    info!("Prompt {} contains images, using Claude API directly", prompt_id);
    
    // Use Claude API directly for image-containing prompts
    let session_id_clone = session_id;
    let prompt_id_clone = prompt_id;
    let db_clone = ctx.db.clone();
    let prompt_data_clone = prompt_model.data.clone();
    let system_prompt_clone = system_prompt.clone();
    
    tokio::spawn(async move {
        if let Err(e) = process_image_prompt_with_api(
            session_id_clone,
            prompt_id_clone,
            &db_clone,
            &prompt_data_clone,
            &system_prompt_clone,
        ).await {
            error!("Failed to process image prompt {}: {}", prompt_id_clone, e);
        }
    });
    
} else {
    info!("Prompt {} is text-only, using Claude Code CLI", prompt_id);
    
    // Use existing Claude Code CLI approach
    tokio::spawn(async move {
        // ... existing CLI code ...
    });
}

/// Check if prompt data contains images
fn contains_images(data: &serde_json::Value) -> bool {
    if let Some(content) = data.get("content") {
        if let Some(blocks) = content.as_array() {
            for block in blocks {
                if let Some(block_type) = block.get("type").and_then(|v| v.as_str()) {
                    if block_type == "image" {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Process prompt with images using Claude API directly
async fn process_image_prompt_with_api(
    session_id: uuid::Uuid,
    prompt_id: uuid::Uuid,
    db: &DatabaseConnection,
    prompt_data: &serde_json::Value,
    system_prompt: &str,
) -> Result<(), Error> {
    use reqwest::Client;
    use futures::StreamExt;
    
    info!("Processing image prompt {} via Claude API", prompt_id);
    
    // Get Anthropic API key
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| Error::Failed("ANTHROPIC_API_KEY not set".into()))?;
    
    // Extract content blocks (already in correct format)
    let content = prompt_data.get("content")
        .ok_or_else(|| Error::Failed("Missing content field".into()))?;
    
    // Construct the API request
    let request_body = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 8192,
        "system": system_prompt,
        "messages": [{
            "role": "user",
            "content": content
        }],
        "stream": true
    });
    
    info!("Sending request to Claude API for prompt {}", prompt_id);
    
    let client = Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to send request to Claude API: {}", e);
            Error::Failed(Box::new(e))
        })?;
    
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Claude API error: {} - {}", status, body);
        return Err(Error::Failed(format!("Claude API error: {}", status).into()));
    }
    
    // Process streaming response
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut message_count = 0;
    
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| {
            error!("Error reading stream: {}", e);
            Error::Failed(Box::new(e))
        })?;
        
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);
        
        // Process complete lines
        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim();
            buffer = buffer[newline_pos + 1..].to_string();
            
            if line.is_empty() || !line.starts_with("data: ") {
                continue;
            }
            
            let json_str = &line[6..]; // Remove "data: " prefix
            
            if json_str == "[DONE]" {
                info!("Stream completed for prompt {}", prompt_id);
                break;
            }
            
            // Parse SSE event and convert to our message format
            match serde_json::from_str::<serde_json::Value>(json_str) {
                Ok(event) => {
                    // Convert Anthropic SSE format to our message format
                    let message_data = convert_anthropic_event_to_message(&event);
                    
                    // Save to database
                    let message_id = uuid::Uuid::new_v4();
                    let new_message = message::ActiveModel {
                        id: Set(message_id),
                        prompt_id: Set(prompt_id),
                        data: Set(message_data),
                        created_at: NotSet,
                        updated_at: NotSet,
                    };
                    
                    if let Err(e) = new_message.insert(db).await {
                        error!("Failed to save message {}: {}", message_id, e);
                    } else {
                        message_count += 1;
                        if message_count % 10 == 0 {
                            info!("Saved {} messages for prompt {}", message_count, prompt_id);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to parse SSE event: {} - {}", e, json_str);
                }
            }
        }
    }
    
    info!("Completed processing image prompt {} ({} messages)", prompt_id, message_count);
    Ok(())
}

/// Convert Anthropic API SSE event to our message format
fn convert_anthropic_event_to_message(event: &serde_json::Value) -> serde_json::Value {
    // Anthropic sends events like:
    // {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "..."}}
    // We need to convert this to a format similar to Claude Code CLI output
    
    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    
    match event_type {
        "message_start" => {
            serde_json::json!({
                "type": "message_start",
                "message": event.get("message").cloned().unwrap_or(serde_json::json!({}))
            })
        }
        "content_block_start" => {
            serde_json::json!({
                "type": "content_block_start",
                "index": event.get("index"),
                "content_block": event.get("content_block")
            })
        }
        "content_block_delta" => {
            serde_json::json!({
                "type": "content_block_delta",
                "index": event.get("index"),
                "delta": event.get("delta")
            })
        }
        "content_block_stop" => {
            serde_json::json!({
                "type": "content_block_stop",
                "index": event.get("index")
            })
        }
        "message_delta" => {
            serde_json::json!({
                "type": "message_delta",
                "delta": event.get("delta"),
                "usage": event.get("usage")
            })
        }
        "message_stop" => {
            serde_json::json!({
                "type": "message_stop"
            })
        }
        _ => event.clone()
    }
}
```

**Add to `Cargo.toml`**:
```toml
reqwest = { version = "0.11", features = ["stream", "json"] }
futures = "0.3"
```

### Phase 3: Testing & Documentation (Week 3)

#### Integration Test

**File**: `tests/image_prompt_test.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose};

    const TINY_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    #[tokio::test]
    async fn test_validate_prompt_with_image() {
        let data = serde_json::json!({
            "content": [
                {
                    "type": "text",
                    "text": "Analyze this image"
                },
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": TINY_PNG_BASE64
                    }
                }
            ]
        });

        // Should pass validation
        let result = validate_prompt_data(&data);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_prompt_too_many_images() {
        let mut blocks = vec![
            serde_json::json!({"type": "text", "text": "Test"})
        ];

        // Add 6 images (exceeds limit of 5)
        for _ in 0..6 {
            blocks.push(serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": TINY_PNG_BASE64
                }
            }));
        }

        let data = serde_json::json!({"content": blocks});

        // Should fail validation
        let result = validate_prompt_data(&data);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validate_prompt_invalid_media_type() {
        let data = serde_json::json!({
            "content": [{
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/bmp",  // Not supported
                    "data": TINY_PNG_BASE64
                }
            }]
        });

        let result = validate_prompt_data(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_contains_images() {
        let data_with_image = serde_json::json!({
            "content": [
                {"type": "text", "text": "Hello"},
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "..."
                    }
                }
            ]
        });

        assert!(contains_images(&data_with_image));

        let data_text_only = serde_json::json!({
            "content": "Just text"
        });

        assert!(!contains_images(&data_text_only));
    }
}
```

#### API Documentation

**Add to OpenAPI schema annotations**:

```rust
/// Create a new prompt
///
/// # Request Body
///
/// Accepts prompts in multiple formats:
///
/// ## Text-only (legacy):
/// ```json
/// {
///   "session_id": "uuid",
///   "data": {
///     "content": "Fix the bug in auth.rs"
///   }
/// }
/// ```
///
/// ## With images (new):
/// ```json
/// {
///   "session_id": "uuid",
///   "data": {
///     "content": [
///       {
///         "type": "text",
///         "text": "Fix the bug shown in this screenshot"
///       },
///       {
///         "type": "image",
///         "source": {
///           "type": "base64",
///           "media_type": "image/png",
///           "data": "iVBORw0KGgo..."
///         }
///       }
///     ]
///   }
/// }
/// ```
///
/// # Constraints
/// - Maximum total size: 10MB
/// - Maximum images per prompt: 5
/// - Supported formats: image/jpeg, image/png, image/gif, image/webp
/// - Images must be base64-encoded
///
#[openapi]
#[post("/prompts", data = "<input>")]
pub async fn create(...) { ... }
```

### Phase 4: Client Examples

#### TypeScript Example

**File**: `sdk/examples/image-prompt.ts`

```typescript
import { Client } from '@wholelottahoopla/prompt-backend-client';
import * as fs from 'fs';
import * as path from 'path';

async function createImagePrompt() {
  const client = new Client({
    basePath: 'https://api.example.com',
    token: process.env.AUTH_TOKEN
  });

  // Read an image file
  const imagePath = path.join(__dirname, 'error-screenshot.png');
  const imageBuffer = fs.readFileSync(imagePath);
  const imageBase64 = imageBuffer.toString('base64');

  // Create prompt with image
  const response = await client.prompts.create({
    session_id: 'your-session-uuid',
    data: {
      content: [
        {
          type: 'text',
          text: 'This error appears when I try to log in. Can you identify the issue and suggest a fix?'
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

  console.log('Prompt created:', response.id);
}

createImagePrompt().catch(console.error);
```

#### cURL Example

```bash
#!/bin/bash

# Read image and convert to base64
IMAGE_BASE64=$(base64 -w 0 error-screenshot.png)

# Create prompt with image
curl -X POST https://api.example.com/prompts \
  -H "Authorization: Bearer $AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d @- <<EOF
{
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "data": {
    "content": [
      {
        "type": "text",
        "text": "Analyze this error and suggest a fix"
      },
      {
        "type": "image",
        "source": {
          "type": "base64",
          "media_type": "image/png",
          "data": "$IMAGE_BASE64"
        }
      }
    ]
  }
}
EOF
```

## Deployment Checklist

### Environment Variables

Add to deployment configuration:

```bash
# Already exists
ANTHROPIC_API_KEY=sk-ant-...

# No new variables needed!
```

### Database

No migration required - existing JSONB column handles the new format.

### Monitoring

Add metrics:

```rust
// In validation
counter!("prompts.created.with_images", 1);
histogram!("prompts.image_size_total_bytes", total_size as f64);

// In processing
counter!("prompts.processing.via_api", 1);
counter!("prompts.processing.via_cli", 1);
histogram!("prompts.api_processing_duration_ms", duration.as_millis() as f64);
```

### Feature Flag (Optional)

For safer rollout:

```rust
fn is_image_support_enabled() -> bool {
    std::env::var("ENABLE_IMAGE_PROMPTS")
        .map(|v| v == "true")
        .unwrap_or(false)
}
```

## MVP Success Criteria

- ✅ Users can submit prompts with up to 5 images
- ✅ Claude API processes images correctly
- ✅ Output format matches existing CLI format
- ✅ Text-only prompts continue to use CLI
- ✅ No breaking changes to existing API
- ✅ <10s total processing time for image prompts
- ✅ >99% API success rate

## Future Enhancements

### Post-MVP Improvements

1. **Object Storage Migration**
   - Move images from JSONB to S3/GCS
   - Use presigned URLs
   - Reduce database size

2. **Image Optimization**
   - Automatic compression before storage
   - Format conversion (e.g., PNG → WebP)
   - Thumbnail generation

3. **Enhanced Claude Code CLI Integration**
   - If Claude Code adds native image support
   - Migrate back to unified CLI approach

4. **Advanced Features**
   - Image annotation support
   - OCR pre-processing
   - Multi-turn conversations with images

## Risk Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| Database size growth | High | 10MB limit, monitoring, future S3 migration |
| API rate limits | Medium | Implement retry logic, rate limiting |
| Format incompatibility | Low | Validation catches issues early |
| Performance degradation | Medium | Async processing, metrics tracking |

## Rollback Plan

If issues occur:

1. **Disable via feature flag**: Set `ENABLE_IMAGE_PROMPTS=false`
2. **API validation rejects images**: Users get clear error message
3. **Existing text prompts unaffected**: Continue working normally
4. **No database migration to rollback**: JSONB format is flexible

## Cost Analysis

### Development
- **Time**: 3 weeks (1 developer)
- **Infrastructure**: $0 (uses existing resources)

### Ongoing Costs
- **API calls**: ~$0.50 per 1000 image-containing messages (vs $0.15 for text)
- **Storage**: ~$0.023/GB/month in PostgreSQL
- **Compute**: Negligible (no image processing)

**Estimated monthly cost increase**: $50-$200 depending on usage

## Conclusion

This MVP provides a pragmatic approach to image support by:

1. **Preserving existing text workflow** - Claude Code CLI continues to work
2. **Adding image support via API** - Direct Claude API integration for images
3. **Maintaining consistency** - Same output format and database schema
4. **Enabling future optimization** - Can migrate to object storage later

The hybrid approach minimizes risk while providing full vision capabilities where needed.

---

**Status**: Ready for Implementation  
**Estimated Effort**: 3 weeks  
**Dependencies**: Anthropic API key (already available)  
**Breaking Changes**: None
