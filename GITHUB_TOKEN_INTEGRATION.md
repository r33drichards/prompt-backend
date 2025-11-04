# GitHub Token Integration via Keycloak

## Overview

This document describes how GitHub access tokens are retrieved from Keycloak's identity provider and used in background jobs for Git operations.

## Architecture

When users authenticate via GitHub through Keycloak, Keycloak stores the GitHub access token. This implementation retrieves and uses that token for automated Git operations in the outbox background job.

```
User Authentication Flow:
┌─────────┐      ┌──────────┐      ┌────────┐      ┌────────┐
│ Browser │─────>│ Keycloak │─────>│ GitHub │─────>│   App  │
└─────────┘      └──────────┘      └────────┘      └────────┘
                      │
                      ├─ Stores GitHub Token
                      └─ Issues JWT with user_id
```

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
         │ Validate JWT via │  │  Keycloak   │   │  PostgreSQL  │
         │   JWKS (user_id) │  │  /broker/   │   │   Database   │
         └──────────────────┘  │github/token │   └──────────────┘
                               │  endpoint   │          │
                               └─────────────┘          │
                                       │                │
                                       ▼                ▼
                               ┌─────────────┐   ┌──────────────┐
                               │GitHub Token │──>│Session Record│
                               └─────────────┘   │ w/ gh_token  │
                                                 └──────────────┘
```

```
Background Job Flow:
┌──────────────────┐
│ Outbox Publisher │
│  (Background)    │
└──────────────────┘
         │
         ├─ Query Active Sessions
         ▼
  ┌─────────────┐
  │ PostgreSQL  │──── Retrieves Session with github_token
  └─────────────┘
         │
         ▼
  ┌─────────────────┐
  │ Sandbox (gh CLI)│──── Uses token for: gh auth, git clone, push, etc.
  └─────────────────┘
```

## Implementation Details

### 1. Keycloak Configuration

**File:** `keycloak/oauth2-realm.json`

The GitHub identity provider is configured with:
- `storeToken: true` - Enables Keycloak to store the GitHub access token
- `defaultScope: "repo,user:email"` - Requests necessary GitHub permissions

```json
{
  "alias": "github",
  "storeToken": true,
  "config": {
    "defaultScope": "repo,user:email"
  }
}
```

### 2. Keycloak Client Module

**File:** `src/auth/keycloak_client.rs`

A dedicated client for retrieving GitHub tokens from Keycloak's broker token endpoint.

**Key Methods:**
- `get_github_token(&self, user_access_token: &str) -> Result<String>`
  - Uses Keycloak's broker token endpoint: `/realms/{realm}/broker/github/token`
  - Requires the user's Keycloak JWT token for authorization
  - Returns the GitHub access token stored by Keycloak

**Endpoint Used:**
```
GET /realms/oauth2-realm/broker/github/token
Authorization: Bearer <keycloak-jwt-token>
```

### 3. Database Schema

**Migration:** `migration/src/m20251104_000001_add_github_token_to_sessions.rs`

Added `github_token` field to the `session` table:

```sql
ALTER TABLE session ADD COLUMN github_token VARCHAR NULL;
```

**Entity Model:** `src/entities/session.rs`

```rust
pub struct Model {
    // ... other fields
    pub github_token: Option<String>,
}
```

### 4. Session Creation Handler

**File:** `src/handlers/sessions.rs`

When a session is created:
1. The `AuthenticatedUserWithToken` guard extracts both user info and the raw JWT
2. A `KeycloakClient` is instantiated
3. The GitHub token is fetched using `get_github_token(&jwt_token)`
4. The token is stored in the session record

**Code Flow:**
```rust
// 1. Extract user and JWT token
user_with_token: AuthenticatedUserWithToken

// 2. Fetch GitHub token from Keycloak
let keycloak_client = KeycloakClient::new()?;
let github_token = keycloak_client
    .get_github_token(&user_with_token.token)
    .await?;

// 3. Store in session
let new_session = session::ActiveModel {
    github_token: Set(Some(github_token)),
    // ...
};
```

### 5. Outbox Background Job

**File:** `src/bg_tasks/outbox_publisher.rs`

The background job:
1. Queries active sessions from PostgreSQL
2. Retrieves the stored `github_token` from the session
3. Uses the token to authenticate `gh` CLI in the sandbox
4. Performs Git operations (clone, checkout, commit, push)

**Code Flow:**
```rust
// 1. Get session with GitHub token
let session = Session::find()
    .filter(session::Column::InboxStatus.eq(InboxStatus::Active))
    .one(&db)
    .await?;

// 2. Authenticate with GitHub
if let Some(ref github_token) = session.github_token {
    let auth_command = format!("echo '{}' | gh auth login --with-token", github_token);
    sbx.exec_command(&auth_command).await?;

    // 3. Perform Git operations
    sbx.exec_command("git clone https://github.com/{repo}.git").await?;
    // ...
}
```

## Authentication Flow Details

### Without User Intervention

**Question:** Do users need to install a GitHub App?

**Answer:** **No!** When using Keycloak's GitHub identity provider:

1. **Initial Setup (One-time, by admin):**
   - Register OAuth App in GitHub (already done via `keycloak/configure-github.sh`)
   - Configure OAuth App credentials in Keycloak realm

2. **User Authentication (Each user, once):**
   - User logs into your app
   - Keycloak redirects to GitHub for authentication
   - User authorizes the OAuth app (standard GitHub login)
   - Keycloak receives and **stores** the GitHub access token
   - User is redirected back to your app

3. **Token Retrieval (Automatic, per session):**
   - When user creates a session, backend has their Keycloak JWT
   - Backend calls Keycloak's broker token endpoint
   - Keycloak returns the stored GitHub token
   - Token is saved in session for background job use

4. **Background Job (Automatic, no user interaction):**
   - Job reads GitHub token from session
   - Authenticates `gh` CLI with the token
   - Performs Git operations on behalf of the user

### Key Benefits

✅ **No GitHub App installation required** - Uses standard OAuth App
✅ **No user intervention needed** - Token retrieved automatically
✅ **Secure** - Token stored in Keycloak, not in application code
✅ **Works in background jobs** - Token available for async operations
✅ **Per-user tokens** - Each user's token is their own, with their permissions

## Security Considerations

1. **Token Storage:**
   - GitHub tokens are stored in PostgreSQL session table
   - Consider encrypting tokens at rest for production
   - Tokens are nullable - sessions can exist without GitHub tokens

2. **Token Lifetime:**
   - GitHub OAuth tokens typically don't expire
   - Consider implementing token refresh if using fine-grained tokens
   - Monitor for revoked tokens and handle errors gracefully

3. **Error Handling:**
   - If token fetch fails during session creation, session is still created (token is optional)
   - If token is missing in background job, job fails with clear error message
   - Logs contain warnings for debugging token issues

4. **Permissions:**
   - GitHub token has scopes: `repo, user:email`
   - Token inherits user's GitHub permissions
   - Ensure users understand what permissions they're granting

## Environment Variables

**Required:**
```bash
# Keycloak Configuration
KEYCLOAK_ISSUER=https://keycloak-production-1100.up.railway.app/realms/oauth2-realm
KEYCLOAK_JWKS_URI=https://keycloak-production-1100.up.railway.app/realms/oauth2-realm/protocol/openid-connect/certs
```

**Note:** No additional environment variables needed for GitHub token retrieval!

## Testing

### 1. Verify Keycloak Configuration

```bash
# Check that storeToken is enabled
cat keycloak/oauth2-realm.json | jq '.identityProviders[] | select(.alias=="github") | .storeToken'
# Should output: true

# Check GitHub scopes
cat keycloak/oauth2-realm.json | jq '.identityProviders[] | select(.alias=="github") | .config.defaultScope'
# Should output: "repo,user:email"
```

### 2. Test Session Creation

```bash
# Create a session (requires authenticated user)
curl -X POST https://your-backend.com/sessions \
  -H "Authorization: Bearer <keycloak-jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "repo": "owner/repo",
    "target_branch": "main"
  }'
```

**Expected:** Session created with `github_token` populated in database.

### 3. Verify Token in Database

```sql
-- Check that session has GitHub token
SELECT id, user_id,
       CASE WHEN github_token IS NOT NULL THEN 'Token present' ELSE 'No token' END as token_status
FROM session
WHERE inbox_status = 'active';
```

### 4. Test Background Job

```bash
# Run background job
cargo run -- --bg-tasks outbox-publisher

# Check logs for:
# - "Successfully fetched GitHub token for user <user_id>"
# - "Authenticating with GitHub for session <session_id>"
```

## Troubleshooting

### Token Not Retrieved

**Symptom:** Session created without `github_token`

**Possible Causes:**
1. User not authenticated via GitHub IdP
2. Keycloak `storeToken` is `false`
3. User hasn't linked GitHub account in Keycloak

**Solution:**
- Ensure user logs in via GitHub (not local Keycloak credentials)
- Check Keycloak IdP configuration
- View user in Keycloak admin → Identity Provider Links

### Background Job Fails

**Symptom:** `Error: No GitHub token available for session`

**Possible Causes:**
1. Session created before token integration
2. User authenticated via non-GitHub method
3. Token was not fetched during session creation

**Solution:**
- Check session `github_token` field in database
- Recreate session after user re-authenticates via GitHub
- Check application logs for token fetch errors

### Keycloak Broker Endpoint Returns 404

**Symptom:** `Failed to retrieve IdP token: status=404`

**Possible Causes:**
1. User not linked to GitHub identity provider
2. Invalid identity provider alias
3. Token not stored (storeToken=false)

**Solution:**
- Verify user has linked GitHub account in Keycloak
- Check identity provider alias is "github"
- Ensure `storeToken: true` in realm configuration

## Migration Guide

### Updating Existing Database

1. **Run Migration:**
   ```bash
   cargo run -- migrate
   ```

2. **Existing Sessions:**
   - Old sessions will have `github_token = NULL`
   - Background jobs will fail for these sessions
   - Users must create new sessions after update

3. **User Re-authentication:**
   - Users don't need to re-authenticate if already logged in
   - New sessions will automatically fetch token
   - Token is fetched per-session, not per-login

### Deployment Steps

1. Update Keycloak realm configuration:
   ```bash
   # In keycloak directory
   ./configure-github.sh <client-id> <client-secret>
   docker compose restart keycloak
   ```

2. Deploy backend with new code:
   ```bash
   cargo build --release
   docker compose up -d backend
   ```

3. Run database migrations:
   ```bash
   cargo run -- migrate
   ```

4. Monitor logs for successful token retrieval

## API Changes

### Session Creation Response

**No changes** - Response format remains the same. GitHub token is not exposed in API responses for security.

### Internal Changes Only

All changes are internal:
- Database schema (added field)
- Session entity model (added field)
- Request guard (added `AuthenticatedUserWithToken`)
- Keycloak client (new module)

**No breaking changes to external API!**

## Future Enhancements

1. **Token Refresh:**
   - Implement token refresh for fine-grained tokens
   - Handle expired tokens gracefully

2. **Token Encryption:**
   - Encrypt `github_token` field at rest
   - Use application-level encryption before database storage

3. **Multiple IdP Support:**
   - Support GitLab, Bitbucket, etc.
   - Add `git_provider` field to sessions

4. **Token Revocation Detection:**
   - Detect when user revokes OAuth app access
   - Notify user or mark session as invalid

5. **Admin Endpoints:**
   - Endpoint to refresh GitHub token for a session
   - Endpoint to check token validity

## References

- [Keycloak Identity Brokering](https://www.keycloak.org/docs/latest/server_admin/#_identity_broker)
- [Keycloak Broker Token Endpoint](https://www.keycloak.org/docs/latest/server_admin/#_identity_broker_tokens)
- [GitHub OAuth Apps](https://docs.github.com/en/developers/apps/building-oauth-apps)
- [GitHub CLI Authentication](https://cli.github.com/manual/gh_auth_login)

## Support

For issues or questions:
1. Check application logs (`tracing::info`, `tracing::error`)
2. Verify Keycloak configuration
3. Test token retrieval manually via Keycloak API
4. Review this documentation's Troubleshooting section
