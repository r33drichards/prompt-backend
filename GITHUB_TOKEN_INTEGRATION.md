# GitHub Token Integration via Keycloak

## Overview

Retrieve GitHub tokens from Keycloak in background jobs using Token Exchange.

**Simple:** Background job gets `user_id` from session → Impersonates user via token exchange → Calls broker endpoint → Gets GitHub token → Uses it.

## Flow

```
Background Job:
  ├─ Read session (has user_id)
  ├─ Authenticate as Keycloak admin
  ├─ Token Exchange: Impersonate user
  │  └─ POST /realms/{realm}/protocol/openid-connect/token
  ├─ Use impersonated token to get GitHub token
  │  └─ GET /realms/{realm}/broker/github/token
  ├─ Receive GitHub token
  └─ Use token for gh auth login
```

## Implementation

### KeycloakClient

```rust
// src/auth/keycloak_client.rs

pub async fn get_github_token_for_user(&self, user_id: &str) -> Result<String> {
    // 1. Impersonate user via token exchange
    let user_token = self.impersonate_user(user_id).await?;

    // 2. Call broker endpoint with impersonated token
    let url = format!(
        "{}/realms/{}/broker/github/token",
        self.keycloak_base_url, self.realm
    );

    let response = self.http_client
        .get(&url)
        .bearer_auth(&user_token)
        .send()
        .await?;

    // 3. Return token
    Ok(response.json::<TokenResponse>().await?.access_token)
}
```

### Background Job

```rust
// src/bg_tasks/outbox_publisher.rs

// Fetch token when job runs
let keycloak_client = KeycloakClient::new()?;
let github_token = keycloak_client
    .get_github_token_for_user(&session.user_id)
    .await?;

// Use it
sbx.exec_command("echo '{token}' | gh auth login --with-token").await?;
```

## Configuration

```bash
# Keycloak
KEYCLOAK_URL=https://keycloak.example.com
KEYCLOAK_REALM=oauth2-realm

# Admin credentials (for token exchange)
KEYCLOAK_ADMIN_USERNAME=admin
KEYCLOAK_ADMIN_PASSWORD=<password>

# Client credentials (for token exchange)
KEYCLOAK_CLIENT_ID=<your-client-id>
KEYCLOAK_CLIENT_SECRET=<your-client-secret>

# JWT validation
KEYCLOAK_ISSUER=https://keycloak.example.com/realms/oauth2-realm
KEYCLOAK_JWKS_URI=https://keycloak.example.com/realms/oauth2-realm/protocol/openid-connect/certs
```

## Keycloak Setup

### 1. Enable Token Exchange Feature

For Keycloak < 26.2 (preview feature):
```bash
kc.sh start --features=token-exchange
```

For Keycloak >= 26.2: Token Exchange is officially supported (no feature flag needed).

### 2. Configure Client Permissions

**Grant Impersonation Role:**
- Navigate to: Clients → Your Client → Service Account Roles
- Select "realm-management" from Client Roles dropdown
- Assign "impersonation" role

**Enable Token Exchange on GitHub IdP:**
- Navigate to: Identity Providers → GitHub → Permissions tab
- Toggle "Permissions Enabled" to ON
- Configure policy to allow your client to exchange tokens

### 3. Configure GitHub Identity Provider

**Enable token storage and readable tokens:**

```json
{
  "alias": "github",
  "storeToken": true,
  "config": {
    "defaultScope": "repo,user:email"
  }
}
```

**In Keycloak Admin Console:**
- Navigate to: Identity Providers → GitHub
- Enable "Stored Tokens Readable" switch (this auto-assigns read-token role to users)

## Security

**Tokens stored:**
- ✅ Keycloak (permanent)
- ❌ Database (never)
- ⚠️ Job memory (during execution only)

**Admin credentials:**
- Required for Keycloak admin API access
- Store securely in environment variables
- Consider using dedicated service account

## Testing

```bash
# 1. User authenticates via GitHub through Keycloak

# 2. Create session
curl -X POST http://localhost:8000/sessions \
  -H "Authorization: Bearer <jwt>" \
  -d '{"repo":"owner/repo","target_branch":"main"}'

# 3. Trigger background job
# Job logs will show:
# "Fetching GitHub token for user <user_id>"
# "Successfully retrieved GitHub token for user <user_id>"
```

## Summary

- Background job fetches token on-demand using token exchange
- Uses two-step process: impersonate user, then get external IdP token
- No tokens stored in database
- Token exists in memory only during job execution
- Requires admin credentials and client credentials with impersonation permissions

## Troubleshooting

### "Client not allowed to exchange" (403)
**Solution:** Ensure the client has:
- Impersonation role assigned (realm-management → impersonation)
- Token exchange permissions on the GitHub IdP
- Token exchange feature enabled (if Keycloak < 26.2)

### "Account not linked" (400)
**Solution:** User must have logged in with GitHub at least once to link their account

### "User {user_id} not found" (404)
**Solution:** Verify the user_id from the JWT matches a Keycloak user. Check JWT validation is working correctly.
