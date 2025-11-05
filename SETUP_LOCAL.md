# Local Development Setup with OAuth

This guide will help you run the backend and frontend locally with Keycloak OAuth authentication.

## Prerequisites

- Docker and Docker Compose installed
- Node.js and npm installed
- Nix (for backend development)
- GitHub OAuth App credentials (you'll provide these)

## Services Included

The `docker-compose.yml` sets up:
- **PostgreSQL** (port 5432): Main application database
- **Redis** (port 6379): Cache and job queue
- **Keycloak** (port 8080): OAuth/OIDC provider
- **Keycloak PostgreSQL** (internal): Database for Keycloak

## Quick Start

### 1. Configure GitHub OAuth App

First, create a GitHub OAuth App:

1. Go to https://github.com/settings/developers
2. Click "New OAuth App"
3. Fill in:
   - **Application name**: `Prompt Submission Local Dev`
   - **Homepage URL**: `http://localhost:5173`
   - **Authorization callback URL**: `http://localhost:8080/realms/oauth2-realm/broker/github/endpoint`
4. Click "Register application"
5. Note down your **Client ID** and **Client Secret**

### 2. Configure Keycloak Realm with GitHub Credentials

Run the configuration script with your GitHub credentials:

```bash
cd /Users/robertwendt/workspace/prompt-backend/.worktrees/oauth
./keycloak/configure-github.sh YOUR_GITHUB_CLIENT_ID YOUR_GITHUB_CLIENT_SECRET
```

Example:
```bash
./keycloak/configure-github.sh Iv1.abc123def456 1234567890abcdef1234567890abcdef12345678
```

This will:
- Update the `keycloak/oauth2-realm.json` file with your credentials
- Create a backup at `keycloak/oauth2-realm.json.bak`

### 3. Start Docker Services

```bash
cd /Users/robertwendt/workspace/prompt-backend/.worktrees/oauth
docker compose up -d
```

Wait for all services to be healthy:
```bash
docker compose ps
```

You should see all services with status "Up (healthy)".

### 4. Check Keycloak Startup

Monitor Keycloak logs to ensure the realm imports successfully:
```bash
docker compose logs -f keycloak
```

Look for messages like:
- "Imported realm oauth2-realm from file"
- "Keycloak ... started"

Press `Ctrl+C` to exit logs.

### 5. Run Database Migrations

```bash
cd /Users/robertwendt/workspace/prompt-backend/.worktrees/oauth
nix develop --command cargo run --bin migration up
```

Or run fresh migrations (WARNING: drops all data):
```bash
cd migration
nix develop --command cargo run -- fresh
```

### 6. Start Backend Server

Open a new terminal:
```bash
cd /Users/robertwendt/workspace/prompt-backend/.worktrees/oauth
nix develop --command cargo run
```

The backend will start on **http://localhost:8000**

Endpoints:
- Health: http://localhost:8000/health (no auth required)
- API Docs: http://localhost:8000/swagger-ui/
- OpenAPI: http://localhost:8000/openapi.json

### 7. Start Frontend Dev Server

Open another terminal:
```bash
cd /Users/robertwendt/workspace/Promptsubmissionui/.worktrees/oauth
npm run dev
```

The frontend will start on **http://localhost:5173**

## Testing Authentication

### Option 1: Login with Test User (Username/Password)

1. Go to http://localhost:5173
2. You'll be redirected to Keycloak login
3. Login with:
   - **Username**: `testuser`
   - **Password**: `testpass`
4. You'll be redirected back to the application

### Option 2: Login with GitHub

1. Go to http://localhost:5173
2. You'll be redirected to Keycloak login
3. Click the **"GitHub"** button
4. Authorize the GitHub OAuth app
5. You'll be redirected back to the application

## Admin Access

### Keycloak Admin Console

- URL: http://localhost:8080
- Username: `admin`
- Password: `admin`

Navigate to: **Administration Console** → **oauth2-realm**

Here you can:
- View/edit users
- Check GitHub identity provider configuration
- View client settings
- Monitor sessions

### Database Access

Connect to PostgreSQL directly:
```bash
docker exec -it prompt-backend-postgres psql -U promptuser -d prompt_backend
```

Or use any PostgreSQL client:
- Host: `localhost`
- Port: `5432`
- Database: `prompt_backend`
- Username: `promptuser`
- Password: `promptpass`

### Redis CLI

```bash
docker exec -it prompt-backend-redis redis-cli
```

## Troubleshooting

### Keycloak won't start / realm won't import

Check logs:
```bash
docker compose logs keycloak
```

Common issues:
- **Missing GitHub credentials**: Make sure you ran `configure-github.sh`
- **Port 8080 in use**: Stop other services using port 8080
- **Database not ready**: Wait longer for keycloak-postgres to be healthy

### Backend can't connect to database

1. Check PostgreSQL is running:
   ```bash
   docker compose ps postgres
   ```

2. Verify DATABASE_URL in `.env`:
   ```bash
   cat .env | grep DATABASE_URL
   ```

3. Test connection:
   ```bash
   docker exec prompt-backend-postgres pg_isready -U promptuser -d prompt_backend
   ```

### Frontend authentication fails

1. Check Keycloak is running: http://localhost:8080
2. Verify frontend .env.development:
   ```bash
   cat /Users/robertwendt/workspace/Promptsubmissionui/.worktrees/oauth/.env.development
   ```
3. Check browser console for errors
4. Verify redirect URIs in Keycloak client settings

### GitHub login doesn't work

1. Verify GitHub OAuth App callback URL is exactly:
   ```
   http://localhost:8080/realms/oauth2-realm/broker/github/endpoint
   ```

2. Check GitHub credentials in Keycloak:
   - Go to http://localhost:8080/admin/master/console/#/oauth2-realm/identity-providers
   - Click "github"
   - Verify Client ID and Client Secret

### GitHub token retrieval fails (404 error)

If you see errors like `Failed to fetch GitHub token for user: Unauthorized: Status: 404 Not Found`, this means the system cannot retrieve the stored GitHub token from Keycloak. Here's how to fix it:

**Root Cause**: Users who authenticated BEFORE `storeToken=true` was enabled don't have their tokens stored in Keycloak.

**Solution**:
1. **User must re-authenticate**: Ask the user to:
   - Log out of the application
   - Log back in using the GitHub identity provider
   - This will store their GitHub token in Keycloak

2. **Verify storeToken is enabled**:
   - Go to Keycloak Admin Console: http://localhost:8080
   - Navigate to: **Identity Providers** → **github**
   - Scroll down to **Store Tokens** and ensure it's **ON**
   - If it was OFF, turn it ON and click Save

3. **Check Keycloak version**:
   ```bash
   docker compose logs keycloak | grep "Keycloak"
   ```
   The token retrieval endpoint requires Keycloak 18 or higher. If you're on an older version, upgrade Keycloak.

4. **Verify user has GitHub identity linked**:
   - Go to Keycloak Admin Console
   - Navigate to: **Users** → find the user → **Identity Provider Links** tab
   - You should see a link to "github"
   - If not, the user needs to log in with GitHub

5. **Check backend logs for details**:
   ```bash
   # When running the backend locally
   cargo run -- --server --bg-tasks -A
   ```
   Look for log messages about federated identities and token retrieval

### Backend returns 401 Unauthorized

1. Check you're logged in to the frontend
2. Open browser DevTools → Network tab
3. Check if Authorization header is present on API requests
4. Verify backend logs for JWT validation errors

## Stopping Services

Stop all Docker services:
```bash
cd /Users/robertwendt/workspace/prompt-backend/.worktrees/oauth
docker compose down
```

Stop and remove volumes (WARNING: deletes all data):
```bash
docker compose down -v
```

## Resetting Everything

To start fresh:
```bash
# Stop and remove all containers and volumes
docker compose down -v

# Reconfigure GitHub credentials
./keycloak/configure-github.sh YOUR_CLIENT_ID YOUR_CLIENT_SECRET

# Start services
docker compose up -d

# Wait for Keycloak to be ready
docker compose logs -f keycloak

# Run migrations
nix develop --command cargo run --bin migration fresh
```

## Environment Files

### Backend: `.env`
```bash
KEYCLOAK_ISSUER=http://localhost:8080/realms/oauth2-realm
KEYCLOAK_JWKS_URI=http://localhost:8080/realms/oauth2-realm/protocol/openid-connect/certs
REDIS_URL=redis://127.0.0.1:6379/
DATABASE_URL=postgres://promptuser:promptpass@localhost:5432/prompt_backend
ANTHROPIC_API_KEY=your_anthropic_api_key_here
```

### Frontend: `.env.development`
```bash
VITE_OIDC_AUTHORITY=http://localhost:8080/realms/oauth2-realm
VITE_OIDC_CLIENT_ID=prompt-submission-ui
VITE_OIDC_REDIRECT_URI=http://localhost:5173/authentication/callback
VITE_OIDC_SCOPE=openid profile email
VITE_OIDC_SILENT_REDIRECT_URI=http://localhost:5173/authentication/silent-callback
VITE_BACKEND_URL=http://localhost:8000
```

## Pre-configured Accounts

### Test User (Local Keycloak)
- Username: `testuser`
- Email: `testuser@example.com`
- Password: `testpass`
- First Name: `Test`
- Last Name: `User`

### Admin (Keycloak Admin Console)
- Username: `admin`
- Password: `admin`

## Architecture

```
┌─────────────────┐
│   Browser       │
│ localhost:5173  │
└────────┬────────┘
         │ OIDC Auth Flow
         ▼
┌─────────────────┐
│   Keycloak      │◄─── GitHub OAuth
│ localhost:8080  │
└────────┬────────┘
         │ JWT Tokens
         ▼
┌─────────────────┐      ┌──────────┐
│   Backend API   │─────►│ Postgres │
│ localhost:8000  │      │   :5432  │
└────────┬────────┘      └──────────┘
         │
         ▼
     ┌────────┐
     │ Redis  │
     │ :6379  │
     └────────┘
```

## Next Steps

1. ✅ Configure GitHub OAuth App
2. ✅ Run configuration script
3. ✅ Start Docker services
4. ✅ Run migrations
5. ✅ Start backend
6. ✅ Start frontend
7. ✅ Test authentication

Happy coding! 🚀
