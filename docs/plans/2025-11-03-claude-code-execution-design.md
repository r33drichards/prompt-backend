# Claude Code Execution in Outbox Publisher

**Date:** 2025-11-03
**Status:** Approved
**Author:** AI Assistant

## Overview

Integrate Claude Code CLI execution into the outbox publisher background job to enable automated task processing in sandboxed environments. Claude Code will run locally on the backend server and connect to borrowed sandboxes via MCP configuration.

## Problem Statement

The outbox publisher currently:
1. Borrows sandbox IPs from the allocator
2. Executes git commands to set up repositories
3. Has TODO placeholders for running Claude Code CLI

We need to implement the Claude Code execution step that:
- Runs npx command locally (not in sandbox)
- Connects to sandbox via MCP config
- Captures and stores output in stream-json format
- Returns borrowed IPs when finished
- Runs asynchronously without blocking job processing

## Architecture

### Execution Flow

```
OutboxJob → Setup Git → Spawn Background Task → Return Success
                              ↓
                    Write MCP Config File
                              ↓
                    Run npx (blocking in thread pool)
                              ↓
                    Parse stream-json output
                              ↓
                    Update session.messages
                              ↓
                    Return borrowed IP
```

### Component Design

#### 1. Fire-and-Forget Task Pattern

Using `tokio::spawn` (Rust equivalent of Go routines):
- Spawns lightweight async task
- Job processor returns immediately
- Background task handles long-running npx command

#### 2. Blocking Command Execution

Using `tokio::task::spawn_blocking` + `std::process::Command`:
- Runs blocking I/O in dedicated thread pool
- Captures full stdout/stderr when process completes
- Simple error handling with `Result<Output, std::io::Error>`

#### 3. MCP Configuration

- Extract `mcp_json_string` from borrowed IP response
- Write to temporary file: `/tmp/borrow-{session_id}.mcp-config`
- Pass file path to npx via `--mcp-config` flag
- Cleanup handled by OS (tmp directory)

#### 4. Output Processing

Stream-json format (one JSON object per line):
```json
{"type":"message","content":"..."}
{"type":"tool_use","tool":"..."}
```

Processing strategy:
- Split stdout by newlines
- Parse each line as JSON
- Log via `tracing::info!()` for real-time monitoring
- Extract message data and append to `session.messages`

#### 5. IP Resource Management

Cleanup sequence:
1. npx command completes (success or failure)
2. Parse and store output
3. Call `ip_client.handlers_ip_return()` to release IP
4. Log any errors but don't propagate (fire-and-forget)

## Implementation Details

### Command Arguments

```rust
std::process::Command::new("npx")
    .args([
        "-y", "@anthropic-ai/claude-code",
        "--append-system-prompt",
        "you are running as a disposable task agent with a git repo checked out in a feature branch. when you completed with your task, commit and push the changes upstream",
        "--dangerously-skip-permissions",
        "--print",
        "--output-format=stream-json",
        "--session-id", &uuid::Uuid::new_v4().to_string(),
        "--allowedTools", "WebSearch", "mcp__*", "ListMcpResourcesTool", "ReadMcpResourceTool",
        "--disallowedTools", "Bash", "Edit", "Write", "NotebookEdit", "Read", "Glob", "Grep", "KillShell", "BashOutput", "TodoWrite",
        "-p", "what are your available tools?",  // TODO: Get from job payload
        "--verbose",
        "--strict-mcp-config",
        "--mcp-config", &config_path,
    ])
    .output()
```

### Error Handling Strategy

| Error Type | Handling |
|------------|----------|
| **npx spawn fails** | Log error, don't update session, return IP |
| **Process exits non-zero** | Log stderr, store partial output, return IP |
| **JSON parse fails** | Log raw line, skip that line, continue parsing |
| **DB update fails** | Log error, return IP anyway |
| **IP return fails** | Log error (leak detection in IP allocator) |

### Timeout Considerations

For initial testing: no timeout (rely on npx's internal timeouts)

Future enhancement:
```rust
tokio::time::timeout(
    Duration::from_secs(600),
    tokio::task::spawn_blocking(|| { ... })
).await
```

## Data Flow

### Input (from borrowed IP response)
```json
{
  "mcp_json_string": "{\"mcpServers\":{...}}",
  "api_url": "http://192.168.1.100:8080"
}
```

### Output (to session.messages)
```json
[
  {"role": "user", "content": "what are your available tools?"},
  {"role": "assistant", "content": "I have access to..."}
]
```

## Testing Strategy

### Initial Testing (Hard-coded Config)
1. Use static MCP config file for local testing
2. Run with simple prompt: "what are your available tools?"
3. Verify stream-json output is captured and logged
4. Check session.messages updated in database

### Integration Testing
1. Full flow: borrow IP → setup git → run Claude → return IP
2. Verify IP is returned even on failures
3. Check logs for complete output

## Future Enhancements

1. **Dynamic Prompts**: Get prompt from `job.payload` instead of hard-coded
2. **Timeout Handling**: Add configurable timeout with graceful shutdown
3. **Progress Tracking**: Parse stream-json in real-time, update session status
4. **Error Recovery**: Retry logic for transient failures
5. **Metrics**: Track execution time, success rate, resource usage

## Security Considerations

- MCP config contains sensitive data (written to /tmp)
- `--dangerously-skip-permissions` bypasses safety checks
- Sandboxes should be ephemeral and network-isolated
- No secrets should be in prompts or outputs

## Dependencies

- `tokio` (already in dependencies)
- `uuid` (already in use)
- No new crates required

## Rollout Plan

1. Implement in `src/bg_tasks/outbox_publisher.rs`
2. Test locally with hard-coded MCP config
3. Deploy to staging with real IP allocator
4. Monitor logs for stream-json output
5. Validate session.messages persistence
6. Enable for production sessions
