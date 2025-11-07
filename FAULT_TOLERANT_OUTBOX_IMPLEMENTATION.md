# Fault-Tolerant and Idempotent Outbox Publisher Implementation

## Overview

This document describes the implementation of a fault-tolerant and idempotent outbox publisher for processing prompts. The changes ensure that prompt processing is reliable, can recover from failures, and will not duplicate work when retried.

## Key Changes

### 1. Database Schema Changes

**Migration**: `m20251107_000002_add_processing_tracking_to_prompt.rs`

Added new fields to the `prompt` table to track processing state:

- `processing_attempts` (integer, default 0): Tracks how many times processing has been attempted
- `last_error` (text, nullable): Stores the last error message if processing failed
- `last_attempt_at` (timestamp, nullable): Records when the last processing attempt occurred
- `completed_at` (timestamp, nullable): Records when processing successfully completed

**Updated Entity**: `src/entities/prompt.rs`

- Added new fields to the `Model` struct
- Added new `Failed` status to `InboxStatus` enum for permanently failed prompts

### 2. Fault-Tolerant Outbox Publisher

**File**: `src/bg_tasks/outbox_publisher.rs`

Complete rewrite with the following improvements:

#### Idempotency Features

1. **Completion Check**: Before processing, checks if prompt is already `Completed` and skips if so
2. **Retry Limit**: Enforces maximum retry attempts (3 by default) to prevent infinite loops
3. **Idempotent Operations**: 
   - Repository clone checks if directory already exists before cloning
   - Branch checkout operations use `git checkout || git switch -c` pattern
   - All operations can be safely retried without side effects

#### Fault Tolerance Features

1. **Error Classification**: Errors are categorized as:
   - `Transient`: Network issues, timeouts - can be retried
   - `Permanent`: Invalid data, missing resources - should not be retried
   - `DatabaseError`: Database connectivity issues - can be retried

2. **Structured Error Handling**:
   - Each step wrapped in error handling with context
   - Errors are logged with structured information
   - Failed prompts are marked with error details in database

3. **Processing Attempt Tracking**:
   - Increments `processing_attempts` counter on each attempt
   - Records `last_attempt_at` timestamp
   - Stores error messages in `last_error` field

4. **Guaranteed Cleanup**:
   - IP return always happens via session status update to `ReturningIp`
   - Status updates occur even if Claude CLI fails
   - Uses separate async task with error handling

5. **State Persistence**:
   - Processing state stored in database after each step
   - Can resume from failures
   - Provides visibility into processing progress

#### Processing Flow

```
1. Parse and validate prompt ID
2. Acquire prompt from database
3. Check if already completed (idempotency)
4. Check if max retries exceeded (fault tolerance)
5. Increment processing attempt counter
6. Get session information
7. Extract prompt content
8. Get sandbox configuration (from pre-allocated IP)
9. Create sandbox client
10. Setup git authentication (with retry)
11. Clone repository (idempotent - skip if exists)
12. Checkout branches (idempotent)
13. Spawn Claude CLI with guaranteed cleanup
    - On success: Mark as Completed
    - On failure: Mark as Failed with error message
    - Always: Update session to ReturningIp
```

### 3. Updated Handlers

**File**: `src/handlers/prompts.rs`

Updated `create` endpoint to initialize new tracking fields with default values:
- `processing_attempts`: 0
- `last_error`: None
- `last_attempt_at`: None
- `completed_at`: None

## Configuration

### Environment Variables

No new environment variables required. Uses existing:
- `GITHUB_TOKEN`: For git authentication
- `DATABASE_URL`: For database connection
- `TMPDIR` or `TEMP_DIR` or `HOME`: For temporary directory location

### Constants

- `MAX_RETRY_ATTEMPTS`: Set to 3 in `outbox_publisher.rs`

## Benefits

### Idempotency

1. **No Duplicate Processing**: Prompts marked as `Completed` are skipped on retry
2. **Safe Retries**: All operations check state before executing
3. **No Resource Leaks**: Repository clones check for existence first

### Fault Tolerance

1. **Automatic Retry**: Transient failures automatically retried by Apalis
2. **Error Visibility**: Failed prompts marked with error details for debugging
3. **Resource Cleanup**: IPs always returned even on failure
4. **Graceful Degradation**: Permanent failures marked as `Failed` instead of infinite retries

### Observability

1. **Processing Metrics**: Track attempt count per prompt
2. **Error Logging**: Structured error messages with context
3. **Timestamps**: Track when processing started and completed
4. **Status Tracking**: Clear status progression: Pending → Active → Completed/Failed

## Testing Considerations

### Test Scenarios

1. **Happy Path**: Prompt processes successfully on first attempt
2. **Transient Failure**: Network error, retry succeeds
3. **Permanent Failure**: Invalid session ID, marked as failed
4. **Max Retries**: 3 failures lead to permanent failure
5. **Duplicate Processing**: Prompt already completed, skipped
6. **Repository Exists**: Clone skipped if directory exists
7. **Partial Completion**: Git auth succeeds but clone fails, retry from clone step

### Monitoring

Monitor the following metrics:
- `processing_attempts` distribution: Most should be 1
- `Failed` status count: Should be low
- `last_error` patterns: Identify recurring issues
- Time between `last_attempt_at` and `completed_at`: Processing duration

## Migration Path

### Deployment Steps

1. **Run Migration**: Apply `m20251107_000002_add_processing_tracking_to_prompt.rs`
2. **Deploy Code**: New fields will be populated for new prompts
3. **Existing Prompts**: Will have default values (0 attempts, no errors)

### Backward Compatibility

- Existing prompts continue to work
- New fields are nullable or have defaults
- No breaking changes to API

## Future Improvements

1. **Exponential Backoff**: Add delays between retries
2. **Retry Policy Configuration**: Make retry limit configurable
3. **Dead Letter Queue**: Move permanently failed prompts to separate table
4. **Metrics Export**: Expose processing metrics for monitoring
5. **Partial Failure Recovery**: Checkpoint progress within Claude CLI execution
6. **Timeout Configuration**: Make timeouts configurable per operation

## Related Files

- `src/bg_tasks/outbox_publisher.rs`: Main implementation
- `src/entities/prompt.rs`: Entity with new fields
- `src/handlers/prompts.rs`: API handlers
- `migration/src/m20251107_000002_add_processing_tracking_to_prompt.rs`: Schema migration
- `migration/src/lib.rs`: Migration registry

## Backup

Original implementation backed up to:
- `src/bg_tasks/outbox_publisher_backup.rs`
