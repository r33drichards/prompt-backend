# Outbox Publisher Fault Tolerance and Idempotency Improvements

## Summary

Made the outbox publisher background task fault-tolerant and idempotent to handle transient failures and ensure reliable job processing.

## Key Improvements

### 1. Idempotency Check
- Added `check_processing_state()` function to query prompt status
- Job skips processing if prompt is already `Completed` or `Archived`
- Allows recovery from worker crashes by continuing `InProgress` jobs
- Prevents duplicate processing when jobs are retried

**Location**: `src/bg_tasks/outbox_publisher.rs:38-59`

### 2. Exponential Backoff Retry Logic
- Added `execute_sandbox_command_with_retry()` function
- Implements 3 retries with exponential backoff (1s, 2s, 4s)
- Applied to all sandbox commands:
  - GitHub authentication (`gh auth login`, `gh auth setup-git`)
  - Git clone operation
  - Git checkout operations
  - Branch creation/switching

**Location**: `src/bg_tasks/outbox_publisher.rs:90-122`

### 3. Improved Timeout Configuration
- Increased git clone timeout from 30s to 60s for large repositories
- Maintains 30s timeout for other git operations

### 4. Mark Completion Function
- Added `mark_prompt_completed()` helper function
- Ready to be called after successful Claude Code execution
- Updates prompt status to `Completed` for cleanup

**Location**: `src/bg_tasks/outbox_publisher.rs:61-88`

### 5. Enhanced Error Handling
- Better error messages with context
- Logging at different levels (info, warn, error)
- Transient failures logged as warnings, permanent failures as errors

## Fault Tolerance Features

The improved system is now resilient to:

1. **Transient Network Failures**: Automatic retry with exponential backoff
2. **Duplicate Job Processing**: Idempotency check prevents re-processing completed jobs
3. **Worker Crashes**: In-progress jobs can be recovered and continued
4. **Temporary Sandbox Unavailability**: Retry logic handles temporary outages
5. **Large Repository Clones**: Extended timeout prevents premature failures

## Architecture

```
Process Outbox Job
    ↓
Check Processing State (Idempotency)
    ↓
[Already Completed] → Skip
[In Progress] → Continue (Recovery)
[Not Started] → Process
    ↓
Execute Sandbox Commands (with retry)
    ↓
Run Claude Code CLI (async)
    ↓
Mark as ReturningIp
```

## Testing Recommendations

1. Test retry logic with intermittent network issues
2. Test idempotency by submitting duplicate jobs
3. Test recovery by killing workers mid-processing
4. Test large repository clones with extended timeout
5. Monitor logs for retry attempts and recovery scenarios

## Future Enhancements

1. Call `mark_prompt_completed()` after successful Claude Code execution
2. Add database transactions for atomic state updates
3. Implement dead letter queue for permanently failed jobs
4. Add metrics for retry attempts and recovery success rates
5. Consider adding jitter to exponential backoff to prevent thundering herd

## Related Files

- `src/bg_tasks/outbox_publisher.rs` - Main implementation
- `src/entities/prompt.rs` - Prompt entity with InboxStatus enum
- `src/entities/session.rs` - Session entity with SessionStatus enum
