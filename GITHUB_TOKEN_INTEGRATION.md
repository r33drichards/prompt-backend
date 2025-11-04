# GitHub Token Integration via Keycloak

## Overview

Retrieve GitHub tokens from Keycloak in background jobs using admin API.

**Simple:** Background job gets `user_id` from session → Calls Keycloak admin API → Gets GitHub token → Uses it.

## Flow

```
Background Job:
  ├─ Read session (has user_id)
  ├─ Call Keycloak admin API with admin credentials
  ├─ GET /admin/realms/{realm}/users/{user_id}/federated-identity/github/token
  ├─ Receive GitHub token
  └─ Use token for gh auth login
```

## Implementation

### KeycloakClient

```rust
// src/auth/keycloak_client.rs

pub async fn get_github_token_for_user(&self, user_id: &str) -> Result<String> {
    // 1. Get admin token
    let admin_token = self.get_admin_token().await?;

    // 2. Call admin API
    let url = format!(
        "{}/admin/realms/{}/users/{}/federated-identity/github/token",
        self.keycloak_base_url, self.realm, user_id
    );

    let response = self.http_client
        .get(&url)
        .bearer_auth(&admin_token)
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

# Admin credentials (required)
KEYCLOAK_ADMIN_USERNAME=admin
KEYCLOAK_ADMIN_PASSWORD=<password>

# JWT validation
KEYCLOAK_ISSUER=https://keycloak.example.com/realms/oauth2-realm
KEYCLOAK_JWKS_URI=https://keycloak.example.com/realms/oauth2-realm/protocol/openid-connect/certs
```

## Keycloak Setup

**Enable token storage in GitHub IdP:**

```json
{
  "alias": "github",
  "storeToken": true,
  "config": {
    "defaultScope": "repo,user:email"
  }
}
```

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

- Background job fetches token on-demand using admin API
- No tokens stored in database
- Token exists in memory only during job execution
- Requires admin credentials
