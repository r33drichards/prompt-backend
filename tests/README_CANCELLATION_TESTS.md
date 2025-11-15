# Task Cancellation Integration Tests

This document describes the comprehensive integration tests for task cancellation functionality.

## Overview

The task cancellation feature allows users to cancel running sessions. The cancellation process involves two steps:

1. **Request Cancellation**: User requests cancellation via the `/sessions/<id>/cancel` API endpoint, which sets the session's `cancellation_status` to `Requested`.
2. **Enforce Cancellation**: A background task (`cancellation_enforcer`) periodically checks for sessions with `cancellation_status = Requested` and a `process_pid`, kills those processes, and updates the status to `Cancelled`.

## Test File

**Location**: `tests/cancellation_integration_test.rs`

## Test Coverage

The integration tests cover the following scenarios:

### 1. Basic Cancellation Flow

#### `test_cancel_session_marks_as_requested`
- **Purpose**: Verifies that a session can be marked for cancellation
- **Steps**:
  1. Create a session with a process PID
  2. Request cancellation (update status to `Requested`)
  3. Verify cancellation metadata is set correctly
- **Assertions**:
  - `cancellation_status` is set to `Requested`
  - `cancelled_at` timestamp is set
  - `cancelled_by` user ID is recorded
  - Process PID is preserved

#### `test_cancel_session_without_process_pid`
- **Purpose**: Tests cancellation of sessions without a running process
- **Scenario**: Session has no process PID (not yet started or already completed)
- **Assertions**:
  - Cancellation can still be requested
  - Status is updated even without PID

### 2. Cancellation Enforcer Behavior

#### `test_cancellation_enforcer_query`
- **Purpose**: Validates the query used by the cancellation enforcer
- **Query**: Finds sessions with `cancellation_status = Requested` AND `process_pid IS NOT NULL`
- **Assertions**:
  - Sessions with both conditions are found
  - Query matches enforcer's filter logic

#### `test_cancellation_enforcer_marks_as_cancelled`
- **Purpose**: Simulates the enforcer's behavior after killing a process
- **Steps**:
  1. Mark session for cancellation (`Requested`)
  2. Simulate process kill
  3. Update status to `Cancelled`
  4. Clear process PID
  5. Set UI status to `NeedsReview`
- **Assertions**:
  - Final status is `Cancelled`
  - Process PID is cleared
  - UI status transitions correctly

### 3. Edge Cases

#### `test_cancel_already_cancelled_session`
- **Purpose**: Tests idempotency - cancelling an already cancelled session
- **Scenario**: Session status is already `Cancelled`
- **Assertions**:
  - System recognizes session is already cancelled
  - No errors occur when checking cancelled sessions

#### `test_multiple_sessions_cancellation`
- **Purpose**: Tests batch cancellation of multiple sessions
- **Scenario**: 
  - Session 1: Has PID, marked for cancellation
  - Session 2: Has PID, marked for cancellation
  - Session 3: No PID, marked for cancellation
- **Assertions**:
  - Enforcer finds sessions 1 and 2 (have PIDs)
  - Session 3 is NOT picked up by enforcer (no PID)

### 4. State Transitions

#### `test_cancellation_state_transitions`
- **Purpose**: Validates the full lifecycle of session cancellation
- **States**:
  1. **Initial**: No cancellation, `ui_status = InProgress`
  2. **Requested**: `cancellation_status = Requested`, timestamps set
  3. **Cancelled**: `cancellation_status = Cancelled`, `ui_status = NeedsReview`, PID cleared
- **Assertions**: Each state transition is validated

### 5. Data Integrity

#### `test_cancellation_preserves_metadata`
- **Purpose**: Ensures cancellation doesn't corrupt session metadata
- **Metadata Checked**:
  - Repository name
  - Branch name
  - Session title
  - Target branch
  - Sandbox configuration
- **Assertions**: All metadata remains intact after cancellation

### 6. Multi-User Support

#### `test_query_sessions_by_user_and_cancellation_status`
- **Purpose**: Validates user isolation in cancellation queries
- **Scenario**:
  - User 1 has a cancelled session
  - User 2 has a cancelled session
- **Assertions**:
  - Queries filter correctly by user ID
  - Users only see their own cancelled sessions

### 7. Enum Validation

#### `test_cancellation_status_enum_values`
- **Purpose**: Unit test to verify `CancellationStatus` enum integrity
- **Type**: Unit test (no database required)
- **Assertions**: `Requested` and `Cancelled` are distinct values

## Test Patterns

### Database Connection
- Uses `try_create_test_db()` to connect to test database
- Gracefully skips tests if database is unavailable (CI/CD environments)
- Database URL: `postgres://promptuser:promptpass@localhost:5432/prompt_backend_test`

### Helper Functions

#### `create_test_session(db, user_id, process_pid)`
- Creates a test session with configurable PID
- Returns `SessionModel` for further manipulation

#### `cleanup_session(db, session_id)`
- Cleans up test data after each test
- Prevents test pollution

### Macro: `skip_if_no_db!`
- Gracefully skips tests when database is unavailable
- Prints informative message to stderr

## Running Tests

### Run all cancellation tests:
```bash
cargo test --test cancellation_integration_test
```

### Run a specific test:
```bash
cargo test --test cancellation_integration_test test_cancel_session_marks_as_requested
```

### Run with output:
```bash
cargo test --test cancellation_integration_test -- --nocapture
```

### Run all integration tests:
```bash
cargo test --tests
```

## Environment Setup

### Required Environment Variables

- `DATABASE_URL`: PostgreSQL test database connection string
  - Default: `postgres://promptuser:promptpass@localhost:5432/prompt_backend_test`

### Test Database Setup

```bash
# Create test database
createdb prompt_backend_test

# Run migrations on test database
DATABASE_URL=postgres://promptuser:promptpass@localhost:5432/prompt_backend_test cargo run --bin migrate
```

## Test Statistics

- **Total Tests**: 11
- **Async Tests**: 10
- **Unit Tests**: 1
- **Lines of Code**: ~480

## Related Files

- **Implementation**: `src/bg_tasks/cancellation_enforcer.rs`
- **API Handler**: `src/handlers/sessions.rs` (cancel endpoint)
- **Entity Model**: `src/entities/session.rs` (`CancellationStatus` enum)
- **Existing Integration Tests**: `tests/dlq_integration_test.rs` (pattern reference)

## Future Enhancements

Potential additions to test coverage:

1. **End-to-End API Tests**: Test the full HTTP API flow using Rocket's test client
2. **Concurrent Cancellation**: Test race conditions when multiple cancellations occur
3. **Process Kill Failure**: Test behavior when process kill fails
4. **Timeout Tests**: Test enforcer behavior with long-running processes
5. **Metrics/Monitoring**: Validate cancellation metrics are recorded correctly

## Notes

- Tests follow the same pattern as `dlq_integration_test.rs`
- All tests include proper cleanup to avoid side effects
- Tests are designed to run in CI/CD environments without database
- Mock PIDs are used (99999, 88888, etc.) to avoid killing real processes
