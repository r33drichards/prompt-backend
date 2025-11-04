# Quick Reference Card

## 🚀 URLs

| Service | URL | Notes |
|---------|-----|-------|
| **Frontend** | http://localhost:5173 | React app |
| **Backend API** | http://localhost:8000 | Rust API |
| **Health Check** | http://localhost:8000/health | No auth required |
| **API Docs** | http://localhost:8000/swagger-ui/ | Interactive docs |
| **Keycloak** | http://localhost:8080 | OIDC provider |
| **Keycloak Admin** | http://localhost:8080/admin | Admin console |

## 🔐 Credentials

### Test User (Login to App)
```
Username: testuser
Password: testpass
Email:    testuser@example.com
```

### Keycloak Admin
```
Username: admin
Password: admin
URL:      http://localhost:8080/admin
```

### PostgreSQL Database
```
Host:     localhost
Port:     5432
Database: prompt_backend
Username: promptuser
Password: promptpass
```

### Redis
```
Host: localhost
Port: 6379
```

## 🎯 Quick Commands

### Start Everything
```bash
cd /Users/robertwendt/workspace/prompt-backend/.worktrees/oauth

# 1. Configure GitHub (first time only)
./keycloak/configure-github.sh YOUR_CLIENT_ID YOUR_CLIENT_SECRET

# 2. Start Docker services
docker compose up -d

# 3. Run migrations (first time only)
nix develop --command cargo run --bin migration fresh
```

### Run Backend
```bash
cd /Users/robertwendt/workspace/prompt-backend/.worktrees/oauth
nix develop --command cargo run
```

### Run Frontend
```bash
cd /Users/robertwendt/workspace/Promptsubmissionui/.worktrees/oauth
npm run dev
```

### View Logs
```bash
# All services
docker compose logs -f

# Specific service
docker compose logs -f keycloak
docker compose logs -f postgres
docker compose logs -f redis
```

### Stop Everything
```bash
docker compose down          # Stop services
docker compose down -v       # Stop and delete data
```

## 🔧 Database Access

### Via Docker
```bash
docker exec -it prompt-backend-postgres psql -U promptuser -d prompt_backend
```

### Via Connection String
```bash
psql postgres://promptuser:promptpass@localhost:5432/prompt_backend
```

## 📋 GitHub OAuth App Setup

When creating your GitHub OAuth App, use these values:

```
Application name:         Prompt Submission Local Dev
Homepage URL:             http://localhost:5173
Authorization callback:   http://localhost:8080/realms/oauth2-realm/broker/github/endpoint
```

## 🐛 Troubleshooting Quick Checks

### Is everything running?
```bash
docker compose ps
```

### Is Keycloak ready?
```bash
curl http://localhost:8080/health/ready
```

### Can I connect to the database?
```bash
docker exec prompt-backend-postgres pg_isready -U promptuser -d prompt_backend
```

### Is Redis working?
```bash
docker exec prompt-backend-redis redis-cli ping
# Should return: PONG
```

### Check backend health
```bash
curl http://localhost:8000/health
```

## 📦 Service Containers

| Container Name | Image | Port |
|----------------|-------|------|
| prompt-backend-postgres | postgres:15-alpine | 5432 |
| prompt-backend-redis | redis:7-alpine | 6379 |
| keycloak-postgres | postgres:15-alpine | - |
| prompt-backend-keycloak | keycloak:23.0 | 8080 |

## 🔄 Full Reset

```bash
cd /Users/robertwendt/workspace/prompt-backend/.worktrees/oauth

# Stop and delete everything
docker compose down -v

# Reconfigure GitHub
./keycloak/configure-github.sh YOUR_CLIENT_ID YOUR_CLIENT_SECRET

# Start fresh
docker compose up -d
docker compose logs -f keycloak  # Wait for "started"
nix develop --command cargo run --bin migration fresh
```

## 📝 Files to Know

```
backend/.env                          # Backend environment config
frontend/.env.development             # Frontend environment config
keycloak/oauth2-realm.json            # Keycloak realm configuration
keycloak/configure-github.sh          # GitHub credentials setup script
docker-compose.yml                    # All services definition
```
