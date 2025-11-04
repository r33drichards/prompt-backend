# GitHub Token Integration via Keycloak (Simplified)

## Overview

This implementation retrieves GitHub tokens from Keycloak's identity provider using the **simple broker token endpoint**. Tokens are fetched at session creation and passed to background jobs via the job queue.

**Key Design: Tokens stored temporarily in Redis job queue, never in database.**

## Architecture

```
Session Creation Flow:
┌─────────┐                    ┌────────────────┐
│ Client  │──── JWT Token ────>│ Backend API    │
└─────────┘                    └────────────────┘
                                       │
                    ┌──────────────────┼──────────────────┐
                    │                  │                  │
                    ▼                  ▼                  ▼
         ┌──────────────────┐  ┌─────────────┐   ┌──────────────┐
         │ Validate JWT     │  │  Keycloak   │   │  PostgreSQL  │
         │ (get user_id)    │  │  /broker/   │   │   Database   │
         └──────────────────┘  │github/token │   └──────────────┘
                               │  endpoint   │          │
                               └─────────────┘          │
                                       │                │
                                       ▼                ▼
                               ┌─────────────┐   ┌──────────────┐
                               │GitHub Token │   │Session Record│
                               └─────────────┘   │ (no token)   │
                                       │         └──────────────┘
                                       ▼
                               ┌──────────────────┐
                               │  Enqueue Job     │
                               │  (token in       │
                               │   payload)       │
                               └──────────────────┘
                                       │
                                       ▼
                               ┌──────────────────┐
                               │  Redis Queue     │
                               │  (temporary      │
                               │   storage)       │
                               └──────────────────┘
```

```
Background Job Flow:
┌──────────────────┐
│ Outbox Publisher │
└──────────────────┘
         │
         ├─ Read job from Redis (includes github_token)
         ▼
  ┌─────────────────┐
  │ Sandbox (gh CLI)│──── Uses token for: gh auth, git clone, push
  └─────────────────┘
```

## Implementation

### 1. Keycloak Configuration

**File:** `keycloak/oauth2-realm.json`

```json
{
  "alias": "github",
  "storeToken": true,
  "config": {
    "defaultScope": "repo,user:email"
  }
}
```

### 2. Keycloak Client (Simplified!)

**File:** `src/auth/keycloak_client.rs`

**Single endpoint used:**
```
GET /realms/{realm}/broker/{provider}/token
Authorization: Bearer <user-keycloak-jwt>
```

**Method:**
```rust
pub async fn get_github_token(&self, user_keycloak_token: &str) -> Result<String>
```

That's it! No admin API, no federated identities query, just one simple call.

### 3. Session Handler

**File:** `src/handlers/sessions.rs`

```rust
pub async fn create(
    user_with_token: AuthenticatedUserWithToken,  // Has JWT
    db: &State<DatabaseConnection>,
    input: Json<CreateSessionInput>,
) -> OResult<CreateSessionOutput> {
    // Fetch GitHub token using user's JWT
    let github_token = keycloak_client
        .get_github_token(&user_with_token.token)
        .await?;

    // Create session (no token stored in DB)
    let new_session = session::ActiveModel {
        user_id: Set(user_with_token.user.user_id.clone()),
        // ... other fields, NO github_token field
    };

    // TODO: Enqueue background job with github_token
    // (Job payload includes the token)
}
```

### 4. Background Job

**File:** `src/bg_tasks/outbox_publisher.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxJob {
    pub session_id: String,
    pub payload: serde_json::Value,
    pub github_token: Option<String>,  // Token passed from session creation
}

pub async fn process_outbox_job(job: OutboxJob, ctx: Data<OutboxContext>) {
    // Use token from job payload (stored temporarily in Redis)
    if let Some(github_token) = job.github_token {
        sbx.exec_command("echo '{token}' | gh auth login --with-token").await?;
        // ... git operations
    }
}
```

## Environment Variables

**Only these are needed:**

```bash
# Base URL (for API calls)
KEYCLOAK_URL=https://keycloak.example.com

# Realm name
KEYCLOAK_REALM=oauth2-realm

# JWT validation (issuer claim)
KEYCLOAK_ISSUER=https://keycloak.example.com/realms/oauth2-realm

# Public keys (for JWT validation)
KEYCLOAK_JWKS_URI=https://keycloak.example.com/realms/oauth2-realm/protocol/openid-connect/certs
```

**No admin credentials required!** ✅

## Security

**Where tokens exist:**
- ✅ Keycloak (designed for this)
- ✅ Redis job queue (temporary, TTL-based)
- ❌ PostgreSQL database (never)
- ⚠️ Memory during job execution (short-lived)

**Token lifecycle:**
1. User authenticates → Keycloak stores token
2. Session created → Token fetched from Keycloak
3. Job enqueued → Token added to Redis job payload
4. Job processed → Token used, then discarded
5. Redis TTL → Token expires from queue

## Benefits of This Approach

| Feature | Status |
|---------|--------|
| **Simple** | ✅ One API call, one endpoint |
| **Secure** | ✅ No tokens in database |
| **No admin creds** | ✅ Uses user's own JWT |
| **Fast** | ✅ Token fetched once at creation |
| **Scalable** | ✅ Redis handles job queue |

## Comparison to Other Approaches

| Approach | Complexity | Security | Performance |
|----------|------------|----------|-------------|
| **Broker endpoint (this)** | ⭐ Low | ⭐⭐⭐ High | ⭐⭐⭐ Fast |
| Admin API | ⭐⭐⭐ High | ⭐⭐ Medium | ⭐⭐ Slow |
| Database storage | ⭐ Low | ❌ Poor | ⭐⭐⭐ Fast |

## Testing

```bash
# 1. User authenticates via GitHub

# 2. Create session
curl -X POST https://your-backend.com/sessions \
  -H "Authorization: Bearer <keycloak-jwt>" \
  -d '{"repo":"owner/repo","target_branch":"main"}'

# Check logs:
# ✅ "Successfully fetched GitHub token for user ..."
# ✅ "Session created successfully"

# 3. Background job runs
# Check logs:
# ✅ "Using GitHub token from job payload..."
# ✅ Git operations succeed
```

## Troubleshooting

**"User not linked to GitHub"**
→ User must log in via GitHub IdP button (not local credentials)

**"Ensure storeToken=true"**
→ Check `keycloak/oauth2-realm.json`, restart Keycloak after changes

**"No GitHub token in job payload"**
→ Token fetch failed at session creation, check session creation logs

## References

- [Blog post that inspired this](https://blog.please-open.it/posts/external-idp-tokens/)
- [Keycloak Identity Brokering](https://www.keycloak.org/docs/latest/server_admin/#_identity_broker)
- [GitHub OAuth Apps](https://docs.github.com/en/developers/apps/building-oauth-apps)

## Summary

**Less is more!** This implementation uses:
- ✅ 1 Keycloak endpoint (broker token)
- ✅ 0 admin credentials
- ✅ 0 database columns for tokens
- ✅ Simple, secure, fast

Token flow: **Keycloak → Session creation → Redis queue → Background job → Discarded**
