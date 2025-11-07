# Outbox Publisher Fault Tolerance

## Overview

The outbox publisher has been enhanced with comprehensive fault tolerance mechanisms to handle transient failures, permanent errors, and ensure reliable job processing.

## Key Features

### 1. Automatic Retry with Exponential Backoff

All sandbox operations (git clone, checkout, GitHub authentication) now include automatic retry logic:

- **Max Retries**: 3 attempts per operation
- **Backoff Strategy**: Exponential (1s, 2s, 3s delays)
- **Operations Covered**:
  - GitHub authentication (`gh auth login`)
  - Git setup (`gh auth setup-git`)
  - Repository cloning
  - Branch checkout operations

### 2. Dead Letter Queue (DLQ) Integration

Failed jobs are tracked and moved to DLQ after exceeding retry limits:

- **Max Retry Count**: 5 attempts before DLQ
- **DLQ Tracking**: Each failure increments retry count
- **Permanent Failures**: Jobs exceeding max retries are marked as "Archived"
- **Metadata Storage**: DLQ entries include prompt ID, session ID, error details, and timestamps

### 3. Status Tracking

Enhanced status tracking for both prompts and sessions:

#### Prompt Status Transitions:
- `Pending` → `Active` (when processing starts)
- `Active` → `Completed` (on success)
- `Active` → `Archived` (after max retries exceeded)

#### Session Status Transitions:
- `Active` → `ReturningIp` (when Claude CLI completes)
- `Active` → `Archived` (on permanent failure)
- Status messages include error details and retry counts

### 4. Error Classification

Errors are classified into two categories:

#### Retriable Errors (Error::Failed)
- Network timeouts
- Sandbox communication failures
- Transient git errors
- Database connection issues

These errors trigger retry logic and increment DLQ counters.

#### Permanent Errors (Error::Abort)
- Invalid UUID format
- Missing required configuration
- Invalid job structure

These errors are logged and the job is immediately aborted without retries.

### 5. Safe Background Task Execution

The Claude Code CLI execution runs in a fire-and-forget background task with:

- Isolated error handling
- Safe database updates
- Proper cleanup on failure
- Comprehensive logging

## Architecture

```
process_outbox_job (entry point)
├── Parse and validate input → Abort on invalid data
├── process_outbox_job_core
│   ├── Query prompt and session
│   ├── Update prompt status to Active
│   ├── Execute sandbox operations (with retry)
│   │   ├── execute_sandbox_command_with_retry
│   │   │   └── 3 attempts with exponential backoff
│   │   ├── GitHub authentication
│   │   ├── Git clone
│   │   ├── Git checkout
│   │   └── Branch creation
│   └── spawn_claude_cli_task (background)
│       ├── Create temp directory
│       ├── Write MCP config
│       ├── run_claude_cli
│       └── update_session_status_safe
└── handle_job_failure (on error)
    ├── get_dlq_retry_count
    ├── Check if max retries exceeded
    ├── Update prompt/session status
    └── Insert DLQ entry if needed
```

## Configuration

### Constants

```rust
const MAX_RETRIES: u32 = 3;              // Retries per operation
const RETRY_DELAY_MS: u64 = 1000;        // Base delay between retries
const MAX_DLQ_RETRY_COUNT: i32 = 5;      // Max job retries before DLQ
```

### Environment Variables

- `GITHUB_TOKEN`: Required for repository operations
- `TMPDIR` or `TEMP_DIR`: Optional, specifies temp directory location
- `DATABASE_URL`: Database connection string

## Failure Scenarios and Handling

### Scenario 1: Transient Network Failure

**Symptom**: Git clone fails due to network timeout

**Handling**:
1. Operation retries 3 times with backoff
2. If all retries fail, job returns `Error::Failed`
3. Apalis reschedules the job
4. DLQ retry counter increments
5. After 5 total failures → moved to DLQ and archived

### Scenario 2: Invalid Configuration

**Symptom**: Session missing `sbx_config`

**Handling**:
1. Error detected early in `process_outbox_job_core`
2. Error propagated to `process_outbox_job`
3. `handle_job_failure` called immediately
4. Status updated, DLQ entry created
5. Job marked for manual review

### Scenario 3: Claude CLI Crash

**Symptom**: Claude CLI process exits with non-zero status

**Handling**:
1. Exit status logged
2. Session status updated to `ReturningIp`
3. IP return poller handles cleanup
4. Job itself marked as complete (setup succeeded)
5. Manual review via logs

### Scenario 4: Database Connection Lost

**Symptom**: Cannot insert message records

**Handling**:
1. Individual message insert failures logged
2. CLI continues processing
3. Other messages still inserted
4. Lost messages can be recovered from CLI session logs

## Monitoring and Observability

### Log Levels

- `info`: Normal operations, retry successes
- `warn`: Retry attempts, recoverable failures
- `error`: Permanent failures, DLQ movements

### Key Metrics to Monitor

1. **Job Success Rate**: `successful_jobs / total_jobs`
2. **DLQ Rate**: `dlq_entries / total_jobs`
3. **Average Retry Count**: Track retry patterns
4. **Operation-Specific Failures**: Which operations fail most often

### Log Patterns

```
INFO Processing outbox job for prompt_id: {uuid}
WARN GitHub auth login failed on attempt 2/3: {error}
INFO GitHub auth login succeeded on retry attempt 3
ERROR Prompt {uuid} exceeded max retries (5), moving to DLQ permanently
```

## Testing Fault Tolerance

### Manual Testing

1. **Test Network Failures**:
   ```bash
   # Simulate network issues
   # Observe retry behavior in logs
   ```

2. **Test Invalid Data**:
   ```bash
   # Insert job with invalid prompt_id
   # Verify Error::Abort behavior
   ```

3. **Test DLQ Integration**:
   ```bash
   # Create failing job
   # Monitor DLQ table for entry after 5 failures
   ```

### Integration Tests

See `tests/dlq_integration_test.rs` for DLQ testing patterns.

## Recovery Procedures

### Reprocessing DLQ Entries

1. Query DLQ for pending entries:
   ```sql
   SELECT * FROM dead_letter_queue 
   WHERE task_type = 'outbox_job' 
   AND status = 'pending';
   ```

2. Investigate root cause from `last_error` field

3. Fix underlying issue (configuration, network, etc.)

4. Re-enqueue job:
   ```sql
   -- Mark as resolved
   UPDATE dead_letter_queue 
   SET status = 'resolved', 
       resolution_notes = 'Fixed and re-enqueued'
   WHERE id = '{dlq_id}';
   
   -- Create new prompt job
   INSERT INTO prompt ...
   ```

### Manual Session Recovery

If a session is stuck:

1. Check session status and status_message
2. Verify IP is properly returned (check `ip_return_poller`)
3. Check DLQ for related entries
4. Manually trigger IP return if needed
5. Create new prompt for retry

## Performance Considerations

### Retry Overhead

- Each retry adds 1-3 seconds delay
- Network operations may timeout (30-60s)
- Total max time per job: ~5 minutes (with all retries)

### DLQ Storage

- DLQ entries are permanent until manually resolved
- Includes full error messages and metadata
- Monitor DLQ table size

### Background Task Isolation

- Claude CLI runs in separate tokio task
- Main job completes after sandbox setup
- Reduces Apalis worker blocking time

## Future Enhancements

1. **Circuit Breaker Pattern**: Stop retrying if service is consistently down
2. **Priority Queue**: Retry failed jobs at lower priority
3. **Retry Delay Configuration**: Make delays configurable per operation type
4. **Metric Instrumentation**: Prometheus metrics for all operations
5. **Automated DLQ Recovery**: Automatic re-enqueue for specific error types
6. **Partial Success Tracking**: Track which operations succeeded before failure

## Related Files

- `src/bg_tasks/outbox_publisher.rs` - Main implementation
- `src/services/dead_letter_queue.rs` - DLQ service layer
- `src/entities/dead_letter_queue.rs` - DLQ entity model
- `src/entities/prompt.rs` - Prompt status tracking
- `src/entities/session.rs` - Session status tracking
- `src/bg_tasks/ip_return_poller.rs` - IP cleanup after job completion

## Contact

For questions or issues related to fault tolerance:
- Check logs first: Look for ERROR and WARN level messages
- Query DLQ: Check dead_letter_queue table
- Review session status: Check session table status_message field
