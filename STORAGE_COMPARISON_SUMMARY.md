# Storage Comparison: PostgreSQL JSONB vs File System

## Quick Answer

### PostgreSQL JSONB Limits
- **Maximum size**: ~1 GB (1,073,741,823 bytes)
- **Practical limit**: 10-100 MB (performance degrades beyond this)
- **Your use case**: Storing base64 images in prompts can easily exceed practical limits

### Recommendation
**Use file system storage** for prompt/message data containing images or large content.

---

## Detailed Comparison

| Aspect | PostgreSQL JSONB | File System | S3 |
|--------|------------------|-------------|-----|
| **Max Size** | 1 GB | Limited by disk | Virtually unlimited |
| **Practical Size** | < 10 MB | < 5 GB per file | < 5 TB per object |
| **Cost** | Expensive DB storage | Cheap local disk | Very cheap |
| **Performance** | Fast for small data | Fast for local access | Moderate (network) |
| **Querying** | Can query JSON fields | No querying | No querying |
| **Backup** | Included in pg_dump | Separate backup needed | Built-in versioning |
| **Durability** | Depends on DB setup | Single point of failure | 99.999999999% |
| **Scalability** | Limited by DB size | Limited by disk | Unlimited |

---

## Cost Analysis

### Current Approach (PostgreSQL JSONB)

**Assumptions**:
- 1000 prompts/day with images
- Average 5 images per prompt × 2 MB each = 10 MB per prompt
- Base64 encoding increases size by 33% = 13.3 MB per prompt
- Retention: 30 days

**Storage needed**:
```
1000 prompts × 13.3 MB × 30 days = 399 GB
```

**Cost** (Railway PostgreSQL):
- Hobby plan: 1 GB included, $0.25/GB after = ~$100/month
- Standard plan: 10 GB included, $0.20/GB after = ~$78/month

### File System Approach

**Local File System** (Railway):
```
399 GB × $0.10/GB = $40/month
```

**S3** (AWS):
```
399 GB × $0.023/GB = $9.18/month
+ Transfer: ~10 GB/day × $0.09/GB = $27/month
= $36.18/month total
```

**Savings**: $42-64/month (42-64% reduction)

---

## Performance Impact

### PostgreSQL JSONB

**Read Performance**:
```
Small (< 1 MB):   ~1-5 ms
Medium (1-10 MB): ~10-50 ms
Large (> 10 MB):  ~100-500 ms ⚠️
```

**Write Performance**:
```
Small (< 1 MB):   ~2-10 ms
Medium (1-10 MB): ~20-100 ms
Large (> 10 MB):  ~200-1000 ms ⚠️
```

**Memory Impact**:
- Entire JSONB value loaded into memory
- 100 concurrent requests × 10 MB = 1 GB RAM usage 💥

### File System (Local)

**Read Performance**:
```
Any size: ~1-10 ms (SSD)
```

**Write Performance**:
```
Any size: ~2-20 ms (SSD)
```

**Memory Impact**:
- Streaming reads possible
- Minimal memory footprint ✅

### File System (S3)

**Read Performance**:
```
Any size: ~50-200 ms (network latency)
```

**Write Performance**:
```
Any size: ~50-200 ms (network latency)
```

---

## Implementation Complexity

| Task | PostgreSQL JSONB | File System |
|------|------------------|-------------|
| **Setup** | ✅ Already done | 🔨 New code needed |
| **Querying** | ✅ Native SQL queries | ❌ No querying (need DB metadata) |
| **Transactions** | ✅ ACID guaranteed | ⚠️ Manual consistency |
| **Backup** | ✅ Included | 🔨 Separate process |
| **Scaling** | ⚠️ Vertical only | ✅ Horizontal possible |
| **Migration** | ✅ No migration | 🔨 Data migration needed |

---

## Recommendation for Your System

### Current State
- `prompt.data`: JSONB storing text + base64 images
- `message.data`: JSONB storing Claude output
- Problem: Images make JSONB too large

### Recommended Architecture

**Hybrid Approach** (best of both worlds):

```
┌──────────────────────────────────────┐
│          Prompt Handler              │
└──────────────┬───────────────────────┘
               │
               ▼
        Is data > 1 MB?
               │
       ┌───────┴───────┐
       │               │
      Yes              No
       │               │
       ▼               ▼
┌─────────────┐  ┌──────────┐
│ File System │  │ Postgres │
│  (images)   │  │  (text)  │
└─────────────┘  └──────────┘
```

**Benefits**:
1. Small prompts (text-only) stay in PostgreSQL → queryable
2. Large prompts (with images) go to file system → performant
3. Automatic routing based on size threshold
4. No breaking changes to API

### Implementation Path

1. **Phase 1** (Week 1): Add storage abstraction layer
   - Implement `BlobStorage` trait
   - Add `LocalFileStorage` backend
   - Add `StorageRouter` for automatic routing
   - No behavior change yet ✅

2. **Phase 2** (Week 2): Database migration
   - Add `data_storage_key`, `storage_backend` columns
   - Deploy with dual-write (both JSONB and file)
   - Monitor for issues ✅

3. **Phase 3** (Week 3): Switch reads
   - Read from file storage first
   - Fallback to JSONB if not found
   - Monitor error rates ✅

4. **Phase 4** (Week 4): Migrate existing data
   - Run batch migration for large prompts
   - Keep small prompts in JSONB
   - Clean up after verification ✅

5. **Phase 5** (Optional): Add S3 support
   - Swap `LocalFileStorage` for `S3Storage`
   - Zero code changes needed ✅

---

## Code Changes Required

### New Files (8 files)
1. `src/storage/mod.rs` - Trait definition
2. `src/storage/local_fs.rs` - Local file system backend
3. `src/storage/s3.rs` - S3 backend (optional)
4. `src/storage/postgres.rs` - PostgreSQL backend wrapper
5. `src/storage/router.rs` - Smart routing logic
6. `src/services/storage_service.rs` - High-level service
7. `migration/src/m20251116_000001_add_external_storage_fields.rs` - Schema migration
8. `tests/integration/storage_test.rs` - Integration tests

### Modified Files (3 files)
1. `src/entities/prompt.rs` - Add storage fields
2. `src/entities/message.rs` - Add storage fields
3. `src/bg_tasks/outbox_publisher.rs` - Use storage service

### Configuration
1. Add environment variables to `.env`
2. Update `Cargo.toml` dependencies

**Total**: ~1500 lines of new code, ~100 lines modified

---

## Risk Assessment

### Low Risk ✅
- Well-tested storage abstraction pattern
- Gradual migration path (no big bang)
- Easy rollback at each phase
- No API changes required

### Medium Risk ⚠️
- Data consistency during migration
- **Mitigation**: Dual-write phase + verification

### High Risk ❌
- None identified

---

## Alternatives Considered

### 1. Stay with PostgreSQL JSONB
**Pros**: No changes needed  
**Cons**: Performance issues, high costs, scaling problems  
**Verdict**: ❌ Not sustainable for image support

### 2. Use object_store Crate
**Pros**: Unified API for S3/Azure/GCS  
**Cons**: Adds complexity, less control  
**Verdict**: ⚠️ Good for multi-cloud, overkill for your needs

### 3. Store Images in Separate Table
**Pros**: Keep using PostgreSQL  
**Cons**: Still same size limits, just different table  
**Verdict**: ❌ Doesn't solve the core problem

### 4. Hybrid Approach (Recommended)
**Pros**: Best performance, lowest cost, flexible  
**Cons**: Requires implementation work  
**Verdict**: ✅ **Best choice**

---

## FAQs

### Q: Can I query prompts with images if they're in file storage?
A: Yes! Store metadata in PostgreSQL (prompt ID, size, created_at), store content in files. Query the metadata table.

### Q: What happens if a file gets corrupted?
A: Implement checksums (SHA-256) stored in database. Verify on read. Keep backups.

### Q: How do I backup file storage?
A: Use S3 versioning, or run periodic rsync to backup location. PostgreSQL metadata table tells you what files exist.

### Q: What about GDPR data deletion?
A: Delete both the file and database record. The storage abstraction makes this transparent.

### Q: Can I switch from local FS to S3 later?
A: Yes! Just run a migration script to copy files from local to S3, then change the config. Zero code changes.

### Q: What if I have existing data in JSONB?
A: Phase 4 migration script handles this. Run it incrementally (100 prompts at a time) to avoid downtime.

---

## Conclusion

**Answer 1**: PostgreSQL JSONB limit is ~1 GB max, ~10 MB practical. Your image use case will exceed this.

**Answer 2**: Yes, file system storage is the right choice. Use the hybrid architecture from `FILESYSTEM_STORAGE_DESIGN.md` for a clean, swappable implementation.

**Next Steps**:
1. Review the design document
2. Decide on storage backend (local FS for dev, S3 for prod)
3. Start with Phase 1 implementation
4. Deploy incrementally

See `FILESYSTEM_STORAGE_DESIGN.md` for full implementation details!
