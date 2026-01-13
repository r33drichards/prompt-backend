# Image Input MVP - Executive Summary

## Quick Overview

This document provides an MVP approach for accepting image inputs in Claude Code prompts through the prompt-backend system.

## The Problem

Currently, the system only accepts text-based prompts. Users cannot attach screenshots, diagrams, or other visual content for Claude Code to analyze when fixing bugs or implementing features.

## The Solution

**Base64-encoded images in JSON** - A simple, infrastructure-light approach that:
- Uses existing database schema (JSONB column)
- Follows Anthropic's Messages API format
- Requires no new services or storage systems
- Maintains backward compatibility

## Example

### Before (Text Only)
```json
{
  "session_id": "...",
  "data": {
    "content": "Fix the authentication bug"
  }
}
```

### After (Text + Image)
```json
{
  "session_id": "...",
  "data": {
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
          "data": "iVBORw0KGgoAAAA..."
        }
      }
    ]
  }
}
```

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **Base64 in JSON** | No new infrastructure, works with existing JSONB column |
| **10MB size limit** | Prevents database bloat while supporting typical screenshots |
| **4 image formats** | JPEG, PNG, GIF, WebP - covers 99% of use cases |
| **Messages API format** | Standard Anthropic format for vision capabilities |
| **Backward compatible** | Text-only prompts continue to work unchanged |

## Implementation Phases

### Phase 1: API Validation (1 week)
- Add input validation for image data
- Implement size and format checks
- Update OpenAPI documentation

### Phase 2: Storage & Processing (1 week)
- Update content extraction logic
- Add image parsing functions
- Implement file handling

### Phase 3: Claude Integration (1 week)
- Connect to Claude Code CLI or API
- Handle image data in sandbox
- Test with real images

### Phase 4: Testing & Documentation (1 week)
- Integration tests
- Performance benchmarks
- Client SDK examples

## Architecture Changes

```
┌─────────────────┐
│  Client (Web)   │
│  Sends JSON     │
│  with base64    │
│  images         │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Rocket API     │
│  Validates      │
│  images & size  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  PostgreSQL     │
│  Stores in      │
│  JSONB column   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Outbox Job     │
│  Extracts       │
│  images, saves  │
│  to temp files  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Claude Code    │
│  Processes      │
│  text + images  │
└─────────────────┘
```

## Benefits

### For Users
- 📸 Attach error screenshots to bug reports
- 📊 Share diagrams for feature requests
- 🎨 Include mockups for UI changes
- 📈 Provide charts for data analysis

### For the System
- ✅ No new infrastructure to maintain
- ✅ Uses existing database and schema
- ✅ Minimal code changes required
- ✅ Easy to rollback if needed

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| **Database size growth** | 10MB limit per prompt, monitoring metrics |
| **Performance impact** | Async processing, temp file cleanup |
| **Security concerns** | Input validation, format verification, rate limiting |
| **Compatibility issues** | Backward compatible design, gradual rollout |

## Metrics to Track

- Number of prompts with images
- Average image size
- Processing time overhead
- Validation failure rate
- Storage usage trends

## Future Enhancements

After MVP is proven in production:

1. **Object Storage** (S3/GCS) for better scalability
2. **Image Optimization** - automatic compression
3. **Advanced Features** - OCR, image annotation
4. **CDN Integration** for faster delivery

## Success Criteria

- ✅ Users can attach images to prompts
- ✅ Claude Code successfully processes images
- ✅ <5s additional latency
- ✅ >99.9% API success rate
- ✅ Zero data corruption

## Cost Analysis

### Development Cost
- **Time**: 4 weeks (1 developer)
- **Infrastructure**: $0 (uses existing resources)
- **Maintenance**: Low (minimal new code)

### Ongoing Costs
- **Storage**: ~$0.023/GB/month in PostgreSQL
- **Compute**: Minimal (temp file I/O only)
- **Monitoring**: Existing Prometheus/Grafana

**Estimated monthly cost increase**: <$50 for typical usage

## Getting Started

For developers implementing this MVP:

1. Read the full design: `IMAGE_INPUT_MVP.md`
2. Review code examples in sections 2-4
3. Follow the 4-phase implementation plan
4. Run the provided unit and integration tests

## Questions?

- **Full technical details**: See `IMAGE_INPUT_MVP.md`
- **API documentation**: Will be updated with Phase 1
- **Client examples**: Included in full design doc

---

**Document Version**: 1.0  
**Date**: November 2025  
**Status**: Design Approved, Ready for Implementation
