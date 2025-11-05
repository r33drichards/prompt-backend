# GitHub Token Integration

## Overview

GitHub tokens are now read from the `GITHUB_TOKEN` environment variable in background jobs.

**Simple:** Background job reads `GITHUB_TOKEN` from environment → Uses it for git operations.

## Flow

```
Background Job:
  ├─ Read session
  ├─ Read GITHUB_TOKEN from environment
  └─ Use token for gh auth login
```

## Implementation

### Background Job

```rust
// src/bg_tasks/outbox_publisher.rs

// Read token from environment
let github_token = std::env::var("GITHUB_TOKEN").map_err(|e| {
    error!("Failed to read GITHUB_TOKEN from environment: {}", e);
    Error::Failed(Box::new(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "GITHUB_TOKEN environment variable not set",
    )))
})?;

// Use it for authentication
sbx.exec_command("echo '{token}' | gh auth login --with-token").await?;
```

## Configuration

```bash
# GitHub Personal Access Token
# Used for git operations and GitHub API access in background tasks
# Required scopes: repo, user:email
GITHUB_TOKEN=your_github_personal_access_token_here
```

Add this to your `.env` file or set it in your deployment environment.

## Security

**Tokens stored:**
- ✅ Environment variables (secure deployment platforms)
- ❌ Database (never)
- ⚠️ Job memory (during execution only)

**Best practices:**
- Use GitHub Personal Access Tokens (PAT) or GitHub App tokens
- Store securely in environment variables
- Limit token scopes to minimum required (repo, user:email)
- Consider using fine-grained tokens for better security
- Rotate tokens regularly

## Testing

```bash
# 1. Set environment variable
export GITHUB_TOKEN=your_github_token_here

# 2. Create session
curl -X POST http://localhost:8000/sessions \
  -H "Authorization: Bearer <jwt>" \
  -d '{"repo":"owner/repo","target_branch":"main"}'

# 3. Trigger background job
# Job logs will show:
# "Reading GitHub token from environment variable"
# "Successfully read GitHub token from environment"
```

## Summary

- Background job reads token from GITHUB_TOKEN environment variable
- No Keycloak integration required for GitHub token
- Token exists in memory only during job execution
- Simple and straightforward implementation
