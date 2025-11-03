# SeaORM Integration

This document describes the SeaORM integration and the session table implementation.

## Database Setup

1. Create a PostgreSQL database for the application.

2. Create a `.env` file in the project root with your database connection string:

```bash
DATABASE_URL=postgres://username:password@localhost/database_name
REDIS_URL=redis://127.0.0.1/
```

3. Run migrations to create the session table:

```bash
cd migration
cargo run
```

Or integrate migrations into your application startup.

## Session Table Schema

The `session` table has the following structure:

- `id` (UUID, Primary Key): Unique identifier for the session
- `messages` (JSONB, Nullable): JSON data for messages
- `inbox_status` (VARCHAR(50)): Status enum with values: pending, active, completed, archived
- `sbx_config` (JSONB, Nullable): Sandbox configuration as JSON
- `parent` (UUID, Nullable): Reference to parent session UUID

## API Endpoints

### Create Session
```
POST /sessions
Content-Type: application/json

{
  "repo": "repository-name",
  "target_branch": "main",
  "messages": { "content": "example" },
  "parent": "uuid-string-or-null"
}
```

Note: `inbox_status` is automatically set to "active", `sbx_config` is set to null, and `title` and `branch` are generated automatically using the Anthropic API.

### Read Session
```
GET /sessions/<id>
```

### List All Sessions
```
GET /sessions
```

### Update Session (PUT - full replacement)
```
PUT /sessions/<id>
Content-Type: application/json

{
  "id": "session-uuid",
  "messages": { "updated": "content" },
  "inbox_status": "active",
  "sbx_config": { "new_config": "value" },
  "parent": "parent-uuid-or-null"
}
```

### Delete Session
```
DELETE /sessions/<id>
```

## InboxStatus Enum Values

- `pending`: Session is pending
- `active`: Session is currently active
- `completed`: Session has been completed
- `archived`: Session has been archived

## Implementation Details

- **ORM**: SeaORM 0.12
- **Database**: PostgreSQL
- **UUID**: Version 4 UUIDs for session IDs
- **JSON Storage**: PostgreSQL JSONB for flexible data storage
