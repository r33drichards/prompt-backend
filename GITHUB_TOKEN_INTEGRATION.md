# GitHub Token Integration via Keycloak Admin API

## Overview

This document describes how GitHub access tokens are retrieved from Keycloak's identity provider **on-demand** using the Keycloak Admin API and used in background jobs for Git operations.

**Key Design Decision:** GitHub tokens are **NOT stored in the application database**. They are retrieved on-demand from Keycloak when needed, ensuring better security and reducing attack surface.

## Architecture

When users authenticate via GitHub through Keycloak, Keycloak stores the GitHub access token. This implementation retrieves tokens on-demand from Keycloak using admin credentials when the background job runs.

```
User Authentication Flow:
┌─────────┐      ┌──────────┐      ┌────────┐      ┌────────┐
│ Browser │─────>│ Keycloak │─────>│ GitHub │─────>│   App  │
└─────────┘      └──────────┘      └────────┘      └────────┘
                      │
                      ├─ Stores GitHub Token (storeToken=true)
                      └─ Issues JWT with user_id
```

```
Session Creation Flow:
┌─────────┐                    ┌────────────────┐
│ Client  │──── JWT Token ────>│ Backend API    │
└─────────┘                    └────────────────┘
                                       │
                                       ▼
                               ┌─────────────┐
                               │ Validate JWT│
                               │ (get user_id)│
                               └─────────────┘
                                       │
                                       ▼
                               ┌──────────────┐
                               │  PostgreSQL  │
                               │Create Session│
                               │(user_id only)│
                               └──────────────┘
```

```
Background Job Flow (On-Demand Token Retrieval):
┌──────────────────┐
│ Outbox Publisher │
│  (Background)    │
└──────────────────┘
         │
         ├─ 1. Query Active Sessions
         ▼
  ┌─────────────┐
  │ PostgreSQL  │──── Retrieves Session with user_id
  └─────────────┘
         │
         ├─ 2. Authenticate as Keycloak Admin
         ▼
  ┌──────────────────────┐
  │ Keycloak Admin API   │
  │ POST /realms/master/ │
  │ protocol/openid-     │
  │ connect/token        │
  └──────────────────────┘
         │
         ├─ 3. Get User's Federated Identity
         ▼
  ┌────────────────────────────┐
  │ Keycloak Admin API         │
  │ GET /admin/realms/{realm}/ │
  │ users/{userId}/            │
  │ federated-identity         │
  └────────────────────────────┘
         │
         ├─ 4. Retrieve GitHub Token
         ▼
  ┌────────────────────────────┐
  │ Keycloak Admin API         │
  │ GET /admin/realms/{realm}/ │
  │ users/{userId}/federated-  │
  │ identity/github/token      │
  └────────────────────────────┘
         │
         ├─ 5. Use Token for Git Operations
         ▼
  ┌─────────────────┐
  │ Sandbox (gh CLI)│──── gh auth, git clone, push, etc.
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

### 2. Keycloak Admin API Client Module

**File:** `src/auth/keycloak_client.rs`

A dedicated client for retrieving GitHub tokens from Keycloak using Admin API.

**Key Methods:**
- `get_admin_token(&self) -> Result<String>`
  - Authenticates with Keycloak using admin credentials
  - Uses `admin-cli` client with password grant

- `get_federated_identities(&self, admin_token, user_id) -> Result<Vec<FederatedIdentity>>`
  - Retrieves list of external identity providers linked to user

- `get_github_token_for_user(&self, user_id: &str) -> Result<String>`
  - Main method used by background jobs
  - Combines the above methods to retrieve GitHub token on-demand

**Endpoints Used:**
```
# 1. Authenticate as admin
POST /realms/master/protocol/openid-connect/token
Body: grant_type=password&client_id=admin-cli&username=admin&password=***

# 2. Get federated identities
GET /admin/realms/{realm}/users/{user_id}/federated-identity
Authorization: Bearer <admin-token>

# 3. Get GitHub token
GET /admin/realms/{realm}/users/{user_id}/federated-identity/github/token
Authorization: Bearer <admin-token>
```

### 3. Database Schema

**No Changes** - The session table does **NOT** store GitHub tokens.

Sessions only store:
- `user_id` (Keycloak subject) - Used to fetch token on-demand
- Other session metadata (repo, branch, etc.)

### 4. Session Creation Handler

**File:** `src/handlers/sessions.rs`

Session creation is simple:
1. Validate user's JWT
2. Extract `user_id` from JWT claims
3. Create session with `user_id`
4. **No token fetching during session creation**

```rust
pub async fn create(
    user: AuthenticatedUser,  // Contains user_id from JWT
    db: &State<DatabaseConnection>,
    input: Json<CreateSessionInput>,
) -> OResult<CreateSessionOutput> {
    // Create session with user_id
    let new_session = session::ActiveModel {
        user_id: Set(user.user_id.clone()),
        // ... other fields, NO github_token field
    };
}
```

### 5. Outbox Background Job

**File:** `src/bg_tasks/outbox_publisher.rs`

The background job:
1. Queries active sessions from PostgreSQL
2. **On-demand**: Creates KeycloakClient and fetches GitHub token using `user_id`
3. Uses the token to authenticate `gh` CLI in the sandbox
4. Performs Git operations (clone, checkout, commit, push)

**Code Flow:**
```rust
// 1. Get session (contains user_id)
let session = Session::find()
    .filter(session::Column::InboxStatus.eq(InboxStatus::Active))
    .one(&db)
    .await?;

// 2. Fetch GitHub token on-demand from Keycloak
let keycloak_client = KeycloakClient::new()?;
let github_token = keycloak_client
    .get_github_token_for_user(&session.user_id)
    .await?;

// 3. Authenticate with GitHub
let auth_command = format!("echo '{}' | gh auth login --with-token", github_token);
sbx.exec_command(&auth_command).await?;

// 4. Perform Git operations
sbx.exec_command("git clone https://github.com/{repo}.git").await?;
// ...
```

## Authentication Flow Details

### Without User Intervention

**Question:** Do users need to install a GitHub App?

**Answer:** **No!** When using Keycloak's GitHub identity provider:

1. **Initial Setup (One-time, by admin):**
   - Register OAuth App in GitHub (already done via `keycloak/configure-github.sh`)
   - Configure OAuth App credentials in Keycloak realm
   - **Configure Keycloak admin credentials** in environment variables

2. **User Authentication (Each user, once):**
   - User logs into your app
   - Keycloak redirects to GitHub for authentication
   - User authorizes the OAuth app (standard GitHub login)
   - Keycloak receives and **stores** the GitHub access token
   - User is redirected back to your app

3. **Session Creation (Per session):**
   - User creates a session via API
   - Backend validates JWT, extracts `user_id`
   - Session created with `user_id` (no token stored)

4. **Background Job (Automatic, on-demand token retrieval):**
   - Job reads `user_id` from session
   - Job authenticates with Keycloak as admin
   - Job retrieves user's GitHub token from Keycloak
   - Authenticates `gh` CLI with the token
   - Performs Git operations on behalf of the user

### Key Benefits

✅ **No GitHub App installation required** - Uses standard OAuth App
✅ **No user intervention needed** - Token retrieved automatically
✅ **Better security** - Tokens NOT stored in application database
✅ **Reduced attack surface** - Only Keycloak stores tokens
✅ **Works in background jobs** - Token retrieved on-demand when needed
✅ **Per-user tokens** - Each user's token is their own, with their permissions
✅ **No token synchronization issues** - Always fetches fresh token from Keycloak

## Security Considerations

1. **Token Storage:**
   - GitHub tokens are **NOT** stored in application database
   - Tokens remain in Keycloak (designed for secure token storage)
   - Tokens retrieved on-demand and exist in memory only during job execution

2. **Admin Credentials:**
   - **CRITICAL:** Keycloak admin credentials must be secured
   - Store in environment variables, never in code
   - Use strong passwords or service accounts
   - Consider using client credentials grant instead of password grant

3. **Admin API Access:**
   - Admin credentials grant broad access to Keycloak
   - Ensure Keycloak is not publicly accessible
   - Use network firewalls to restrict Keycloak access
   - Monitor admin API usage

4. **Token Lifetime:**
   - GitHub OAuth tokens typically don't expire
   - Consider implementing token refresh if using fine-grained tokens
   - Monitor for revoked tokens and handle errors gracefully

5. **Error Handling:**
   - If token fetch fails, background job fails with clear error message
   - Logs contain warnings for debugging token issues
   - User is not notified automatically (admin should monitor logs)

6. **Permissions:**
   - GitHub token has scopes: `repo, user:email`
   - Token inherits user's GitHub permissions
   - Ensure users understand what permissions they're granting

## Environment Variables

**Required:**

```bash
# Keycloak Configuration (existing)
KEYCLOAK_ISSUER=https://keycloak-production-1100.up.railway.app/realms/oauth2-realm
KEYCLOAK_JWKS_URI=https://keycloak-production-1100.up.railway.app/realms/oauth2-realm/protocol/openid-connect/certs

# Keycloak Admin Credentials (NEW - REQUIRED)
KEYCLOAK_ADMIN_USERNAME=admin
KEYCLOAK_ADMIN_PASSWORD=<your-secure-admin-password>
```

**Important:**
- Use the Keycloak master realm admin credentials
- For production, use a dedicated service account with minimal permissions
- Never commit these credentials to version control

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

### 2. Test Admin API Access

```bash
# Get admin token
ADMIN_TOKEN=$(curl -X POST "https://your-keycloak.com/realms/master/protocol/openid-connect/token" \
  -d "grant_type=password" \
  -d "client_id=admin-cli" \
  -d "username=admin" \
  -d "password=yourpassword" | jq -r '.access_token')

# Get user's federated identities
curl "https://your-keycloak.com/admin/realms/oauth2-realm/users/{user-id}/federated-identity" \
  -H "Authorization: Bearer $ADMIN_TOKEN"

# Should return: [{"identityProvider":"github","userId":"...","userName":"..."}]
```

### 3. Test Session Creation

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

**Expected:** Session created with `user_id` in database (no token stored).

### 4. Test Background Job

```bash
# Run background job
cargo run -- --bg-tasks outbox-publisher

# Check logs for:
# - "Fetching GitHub token for user <user_id> from Keycloak"
# - "Authenticating as Keycloak admin"
# - "Fetching federated identities for user <user_id>"
# - "Successfully retrieved github token for user <user_id>"
# - "Authenticating with GitHub for session <session_id>"
```

## Troubleshooting

### Admin Authentication Failed

**Symptom:** `Error: Admin authentication failed`

**Possible Causes:**
1. Invalid admin username/password
2. Admin credentials not for master realm
3. `admin-cli` client disabled

**Solution:**
- Verify `KEYCLOAK_ADMIN_USERNAME` and `KEYCLOAK_ADMIN_PASSWORD`
- Ensure using master realm admin credentials
- Check Keycloak logs for authentication errors

### User Not Linked to GitHub

**Symptom:** `Error: User not found or not linked to GitHub`

**Possible Causes:**
1. User authenticated with local Keycloak credentials (not GitHub)
2. User hasn't linked GitHub account
3. GitHub link was removed

**Solution:**
- Ensure user logs in via GitHub IdP button
- Check user in Keycloak admin → Identity Provider Links
- User may need to re-authenticate via GitHub

### Token Endpoint Returns Error

**Symptom:** `Failed to retrieve IdP token: status=404` or `status=500`

**Possible Causes:**
1. Keycloak `storeToken` is `false`
2. Token wasn't stored during authentication
3. API endpoint not available in Keycloak version

**Solution:**
- Verify `storeToken: true` in realm configuration
- Restart Keycloak after configuration change
- Ensure Keycloak version >= 12 (supports token retrieval)
- Check Keycloak server logs

### Background Job Fails

**Symptom:** `Error: Failed to get GitHub token`

**Possible Causes:**
1. User not authenticated via GitHub
2. Admin credentials invalid
3. Network issues connecting to Keycloak

**Solution:**
- Check background job logs for specific error
- Verify admin credentials
- Test admin API access manually
- Ensure background job can reach Keycloak (network/firewall)

## Comparison: Database Storage vs On-Demand Retrieval

| Aspect | Database Storage (Previous) | On-Demand Retrieval (Current) |
|--------|----------------------------|------------------------------|
| **Security** | ❌ Tokens in app database | ✅ Tokens only in Keycloak |
| **Attack Surface** | ❌ Larger | ✅ Smaller |
| **Token Sync** | ❌ Can become stale | ✅ Always fresh from source |
| **Database Size** | ❌ Larger (stores tokens) | ✅ Smaller |
| **Performance** | ✅ Faster (cached) | ⚠️ Slower (API calls) |
| **Complexity** | ✅ Simpler | ⚠️ Requires admin setup |
| **Best Practice** | ❌ Not recommended | ✅ Recommended |

## Migration Guide

### If You Had Database Storage Before

**This implementation does NOT store tokens in database, so:**

1. **No database migration needed** - No `github_token` column added
2. **No token cleanup needed** - No tokens to remove
3. **Session table unchanged** - Only `user_id` stored

### Deployment Steps

1. **Configure Keycloak admin credentials:**
   ```bash
   # Add to environment variables
   export KEYCLOAK_ADMIN_USERNAME=admin
   export KEYCLOAK_ADMIN_PASSWORD=<secure-password>
   ```

2. **Update Keycloak realm configuration:**
   ```bash
   cd keycloak
   ./configure-github.sh <client-id> <client-secret>
   docker compose restart keycloak
   ```

3. **Deploy backend with new code:**
   ```bash
   cargo build --release
   docker compose up -d backend
   ```

4. **Test token retrieval:**
   - Create a session as a GitHub-authenticated user
   - Trigger background job
   - Monitor logs for successful token retrieval

## API Changes

**No API changes** - This is an internal implementation detail.

- Session creation endpoint unchanged
- Session response format unchanged
- No new endpoints added

## Performance Considerations

### On-Demand Retrieval Overhead

Each background job execution makes 2-3 additional API calls to Keycloak:
1. Admin authentication (~100-200ms)
2. Get federated identities (~50-100ms)
3. Get token (~50-100ms)

**Total overhead:** ~200-400ms per job

**Optimization strategies:**
1. Cache admin token (valid for 60 seconds by default)
2. Batch process multiple sessions with same user
3. Use connection pooling for Keycloak requests

### When to Consider Database Storage

**Only if:**
- Background jobs run very frequently (>100/second)
- Keycloak is geographically distant (>500ms latency)
- You can implement proper token encryption at rest

**For most use cases, on-demand retrieval is preferred for security.**

## Future Enhancements

1. **Admin Token Caching:**
   - Cache admin token in memory (expires after 60s)
   - Reduces API calls to Keycloak

2. **Service Account Instead of Admin:**
   - Create dedicated service account in Keycloak
   - Grant minimal permissions (only user read + token access)
   - More secure than using full admin account

3. **Token Refresh:**
   - Implement token refresh for fine-grained tokens
   - Handle expired tokens gracefully

4. **Multiple IdP Support:**
   - Support GitLab, Bitbucket, etc.
   - Add `git_provider` field to sessions

5. **Token Revocation Detection:**
   - Detect when user revokes OAuth app access
   - Notify user or mark session as invalid

## References

- [Keycloak Identity Brokering](https://www.keycloak.org/docs/latest/server_admin/#_identity_broker)
- [Keycloak Admin REST API](https://www.keycloak.org/docs-api/latest/rest-api/)
- [GitHub OAuth Apps](https://docs.github.com/en/developers/apps/building-oauth-apps)
- [GitHub CLI Authentication](https://cli.github.com/manual/gh_auth_login)

## Support

For issues or questions:
1. Check application logs (`tracing::info`, `tracing::error`)
2. Test admin API access manually
3. Verify Keycloak configuration
4. Review this documentation's Troubleshooting section
