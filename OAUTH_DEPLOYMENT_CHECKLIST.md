# OAuth Deployment Checklist

## Prerequisites
- [ ] Production Keycloak instance is running and accessible
- [ ] Production backend server environment is ready
- [ ] Production frontend hosting is configured
- [ ] SSL/TLS certificates are in place for all services

## 1. Keycloak Configuration (Production)

### 1.1 Create/Configure Realm
- [x] Production realm exists: `oauth2-realm`
- [x] Realm URL: https://keycloak-production-1100.up.railway.app/realms/oauth2-realm

### 1.2 Create Client for Frontend Application
- [x] Client exists (ID: `c6afd469-a23c-4377-af6b-660707f4bdc1`)
- [ ] Verify Client Protocol: `openid-connect`
- [ ] Verify Client Authentication: `OFF` (public client for SPA)
- [ ] Configure Valid Redirect URIs:
  - [ ] Add: `https://promptsubmissionui-production.up.railway.app/*`
  - [ ] Add: `https://promptsubmissionui-production.up.railway.app/authentication/callback`
  - [ ] Add: `https://promptsubmissionui-production.up.railway.app/authentication/silent-callback`
- [ ] Configure Valid Post Logout Redirect URIs:
  - [ ] Add: `https://promptsubmissionui-production.up.railway.app/*`
- [ ] Configure Web Origins:
  - [ ] Add: `https://promptsubmissionui-production.up.railway.app`
- [ ] Save the client

### 1.3 Configure Client Scopes
- [ ] Go to Client Scopes tab of your client
- [ ] Ensure `openid`, `profile`, `email` are included
- [ ] Navigate to the client's dedicated scope (named `{client-id}-dedicated`)

### 1.4 Add Audience Mapper
- [ ] In the dedicated scope, click "Add mapper" → "By configuration"
- [ ] Select "Audience" mapper type
- [ ] Configure mapper:
  - [ ] Name: `prompt-backend-audience`
  - [ ] Included Client Audience: `prompt-backend`
  - [ ] Add to ID token: `ON`
  - [ ] Add to access token: `ON`
- [ ] Save the mapper

### 1.5 Test User Account
- [ ] Create test user account in production Keycloak
- [ ] Set password for test user
- [ ] Verify user can log in to Keycloak directly

### 1.6 Document Keycloak URLs
- [x] Authority URL: `https://keycloak-production-1100.up.railway.app/realms/oauth2-realm`
- [x] JWKS URI: `https://keycloak-production-1100.up.railway.app/realms/oauth2-realm/protocol/openid-connect/certs`
- [x] Issuer: `https://keycloak-production-1100.up.railway.app/realms/oauth2-realm`

## 2. Backend Configuration

### 2.1 Environment Variables
Update production environment variables in Railway:

```bash
# Keycloak Configuration
KEYCLOAK_AUTHORITY=https://keycloak-production-1100.up.railway.app/realms/oauth2-realm
KEYCLOAK_JWKS_URI=https://keycloak-production-1100.up.railway.app/realms/oauth2-realm/protocol/openid-connect/certs
KEYCLOAK_ISSUER=https://keycloak-production-1100.up.railway.app/realms/oauth2-realm

# Database (if changed)
DATABASE_URL=postgresql://user:password@host:port/dbname

# Redis (if changed)
REDIS_URL=redis://host:port

# Server Configuration
ROCKET_ADDRESS=0.0.0.0
ROCKET_PORT=8000
```

- [ ] Set `KEYCLOAK_AUTHORITY` environment variable
- [ ] Set `KEYCLOAK_JWKS_URI` environment variable
- [ ] Set `KEYCLOAK_ISSUER` environment variable
- [ ] Verify all other required environment variables are set

### 2.2 CORS Configuration
Review `src/main.rs` CORS settings:

- [ ] Update CORS to allow only your production frontend URL:
  ```rust
  let cors = CorsOptions::default()
      .allowed_origins(AllowedOrigins::some_exact(&["https://promptsubmissionui-production.up.railway.app"]))
      // ... rest of CORS config
  ```
- [ ] Ensure `allow_credentials(true)` is set

### 2.3 Build and Deploy Backend
- [ ] Merge oauth branch to main (or create production branch)
- [ ] Build production binary: `cargo build --release`
- [ ] Run database migrations on production database
- [ ] Deploy backend to production server
- [ ] Verify backend is accessible at production URL
- [ ] Check backend logs for successful JWKS fetch from Keycloak

### 2.4 Verify Backend Health
- [ ] Test health endpoint (if available)
- [ ] Check logs for "JWKS fetched successfully" message
- [ ] Verify Redis connection
- [ ] Verify database connection

## 3. Frontend Configuration

### 3.1 Environment Variables
Update production frontend environment variables in Railway:

```bash
# OIDC Configuration
VITE_OIDC_AUTHORITY=https://keycloak-production-1100.up.railway.app/realms/oauth2-realm
VITE_OIDC_CLIENT_ID=c6afd469-a23c-4377-af6b-660707f4bdc1
VITE_OIDC_REDIRECT_URI=https://promptsubmissionui-production.up.railway.app/authentication/callback
VITE_OIDC_SILENT_REDIRECT_URI=https://promptsubmissionui-production.up.railway.app/authentication/silent-callback
VITE_OIDC_SCOPE=openid profile email

# Backend API
VITE_BACKEND_URL=https://prompt-backend-production.up.railway.app
```

- [ ] Set `VITE_OIDC_AUTHORITY` in Railway environment variables
- [ ] Set `VITE_OIDC_CLIENT_ID` in Railway environment variables
- [ ] Set `VITE_OIDC_REDIRECT_URI` in Railway environment variables
- [ ] Set `VITE_OIDC_SILENT_REDIRECT_URI` in Railway environment variables
- [ ] Set `VITE_BACKEND_URL` in Railway environment variables

### 3.2 Update OidcTrustedDomains.js
Edit `public/OidcTrustedDomains.js`:

```javascript
const trustedDomains = {
  default: {
    oidcDomains: ['https://keycloak-production-1100.up.railway.app'],
    accessTokenDomains: ['https://prompt-backend-production.up.railway.app'],
  },
};
```

- [ ] Update `oidcDomains` to `https://keycloak-production-1100.up.railway.app`
- [ ] Update `accessTokenDomains` to `https://prompt-backend-production.up.railway.app`
- [ ] Ensure no localhost URLs remain in production build

### 3.3 Build and Deploy Frontend
- [ ] Merge oauth branch to main (or create production branch)
- [ ] Build production bundle: `npm run build`
- [ ] Verify Service Worker files are included in build:
  - [ ] `OidcServiceWorker.js` exists in dist/public
  - [ ] `OidcTrustedDomains.js` exists in dist/public
- [ ] Deploy frontend to production hosting
- [ ] Verify frontend is accessible at production URL

## 4. Integration Testing

### 4.1 Test Authentication Flow
- [ ] Navigate to production frontend URL
- [ ] Click login/sign-in button
- [ ] Verify redirect to Keycloak login page
- [ ] Log in with test user credentials
- [ ] Verify redirect back to frontend after successful login
- [ ] Open browser DevTools → Application → Session Storage
- [ ] Verify OIDC tokens are stored
- [ ] Copy access token and decode at jwt.io
- [ ] Verify token claims:
  - [ ] `iss` matches production Keycloak issuer
  - [ ] `aud` includes "prompt-backend"
  - [ ] `exp` (expiration) is set correctly
  - [ ] `sub` (user ID) is present

### 4.2 Test Service Worker Token Injection
- [ ] Open browser DevTools → Application → Service Workers
- [ ] Verify OidcServiceWorker is active and controlling the page
- [ ] Open DevTools → Network tab
- [ ] Perform an API request to backend (e.g., GET /sessions)
- [ ] Click on the request in Network tab
- [ ] Verify Headers section shows:
  - [ ] `Authorization: Bearer {token}` header is present
  - [ ] Request shows `credentials: include`

### 4.3 Test Backend Authorization
- [ ] Make authenticated API requests from frontend
- [ ] Verify requests return 200 OK (not 401 Unauthorized)
- [ ] Check backend logs for successful token validation messages:
  - [ ] "Token validated successfully for user: {user-id}"
- [ ] Verify no "Token validation failed" errors in logs

### 4.4 Test CORS
- [ ] Verify API requests from frontend succeed (no CORS errors in browser console)
- [ ] Check for CORS-related errors in browser DevTools → Console
- [ ] Verify OPTIONS preflight requests succeed

### 4.5 Test Token Refresh
- [ ] Stay logged in for duration of token lifetime
- [ ] Verify silent token refresh occurs automatically
- [ ] Check for any authentication errors after token expiry time

### 4.6 Test Logout
- [ ] Click logout button in frontend
- [ ] Verify redirect to Keycloak logout or frontend logout page
- [ ] Verify session storage is cleared
- [ ] Verify subsequent API requests return 401 Unauthorized
- [ ] Verify you can log back in successfully

## 5. Security Hardening

### 5.1 Review Security Settings
- [ ] Ensure all communication uses HTTPS (no HTTP)
- [ ] Verify Keycloak is not using default admin credentials
- [ ] Review Keycloak session timeout settings
- [ ] Review Keycloak token lifetime settings (access token, refresh token)
- [ ] Ensure production database credentials are strong and secured
- [ ] Ensure Redis is password-protected or firewalled
- [ ] Review firewall rules to restrict access to backend/database/Redis

### 5.2 Audience Validation
- [ ] Verify backend only accepts tokens with "prompt-backend" audience
- [ ] Test with token from different audience (should fail with 401)

### 5.3 Remove Development Artifacts
- [ ] Remove any debug logging that exposes sensitive data
- [ ] Remove localhost URLs from all configuration files
- [ ] Remove test users if not needed in production
- [ ] Clear any cached JWKS if backend was previously running with different Keycloak

## 6. Monitoring and Logging

### 6.1 Set Up Monitoring
- [ ] Configure logging for backend authentication events
- [ ] Set up alerts for authentication failures
- [ ] Monitor backend error rates
- [ ] Monitor Keycloak availability

### 6.2 Log Review
- [ ] Check backend logs for authentication errors
- [ ] Check Keycloak logs for any issues
- [ ] Verify no sensitive data (tokens, passwords) in logs

## 7. Documentation

### 7.1 Update Documentation
- [ ] Document production Keycloak URLs
- [ ] Document client IDs and configuration
- [ ] Document environment variables required for deployment
- [ ] Update any API documentation with authentication requirements
- [ ] Document rollback procedure if issues occur

### 7.2 Share with Team
- [ ] Share production Keycloak admin access with team (if applicable)
- [ ] Document test user credentials for team
- [ ] Share deployment checklist completion status

## 8. Rollback Plan

### 8.1 Prepare Rollback
- [ ] Document previous working backend version/commit
- [ ] Document previous working frontend version/commit
- [ ] Test rollback procedure in staging environment (if available)
- [ ] Keep previous OAuth-disabled version available for quick rollback

### 8.2 Rollback Steps (if needed)
1. [ ] Revert backend to previous commit: `git checkout {previous-commit}`
2. [ ] Rebuild and redeploy backend
3. [ ] Revert frontend to previous commit: `git checkout {previous-commit}`
4. [ ] Rebuild and redeploy frontend
5. [ ] Notify users of temporary authentication issues

## 9. Post-Deployment Validation

### 9.1 24-Hour Check
- [ ] Monitor error logs for first 24 hours
- [ ] Check for any 401 authentication failures
- [ ] Verify no CORS errors
- [ ] Monitor token refresh behavior
- [ ] Gather user feedback on login experience

### 9.2 Week-Long Check
- [ ] Monitor authentication error rates
- [ ] Review Keycloak audit logs
- [ ] Check for any token expiration issues
- [ ] Verify automatic token refresh is working

## 10. Known Issues and Solutions

### Issue: "No matching routes for OPTIONS"
- **Cause:** CORS preflight failures
- **Solution:** Verify CORS configuration includes OPTIONS method and proper headers

### Issue: "Token validation failed: InvalidAudience"
- **Cause:** Token doesn't include "prompt-backend" in audience claim
- **Solution:** Verify Keycloak audience mapper is configured correctly

### Issue: "Missing Authorization header"
- **Cause:** Service Worker not injecting token
- **Solution:**
  - Verify `credentials: 'include'` in API client configuration
  - Verify OidcTrustedDomains.js includes production backend URL
  - Check Service Worker is active in browser DevTools

### Issue: "JSON error: invalid type: sequence, expected a string"
- **Cause:** Old backend code expecting single audience string
- **Solution:** Ensure latest backend code is deployed (with `deserialize_audience` function)

### Issue: Service Worker not activating
- **Cause:** Browser cache or Service Worker registration issues
- **Solution:**
  - Clear browser cache and storage
  - Hard refresh (Ctrl+Shift+R or Cmd+Shift+R)
  - Unregister old Service Workers in DevTools → Application → Service Workers

---

## Quick Reference

### Production URLs
```
Keycloak Authority: https://keycloak-production-1100.up.railway.app/realms/oauth2-realm
Backend URL: https://prompt-backend-production.up.railway.app
Frontend URL: https://promptsubmissionui-production.up.railway.app
Keycloak Admin: https://keycloak-production-1100.up.railway.app/admin/master/console/#/oauth2-realm
Client ID: c6afd469-a23c-4377-af6b-660707f4bdc1
```

### Test Commands
```bash
# Test backend health
curl https://prompt-backend-production.up.railway.app/health

# Test CORS
curl -X OPTIONS https://prompt-backend-production.up.railway.app/sessions \
  -H "Origin: https://promptsubmissionui-production.up.railway.app" \
  -H "Access-Control-Request-Method: GET" \
  -H "Access-Control-Request-Headers: Authorization" \
  -v

# Test authenticated endpoint (replace TOKEN with actual token)
curl https://prompt-backend-production.up.railway.app/sessions \
  -H "Authorization: Bearer TOKEN" \
  -H "Origin: https://promptsubmissionui-production.up.railway.app"
```

### Deployment Completion
- [ ] All checklist items completed
- [ ] Authentication tested end-to-end
- [ ] Team notified of deployment
- [ ] Monitoring configured
- [ ] Date deployed: `_________________`
- [ ] Deployed by: `_________________`
