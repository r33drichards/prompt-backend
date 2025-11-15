# Multi-Repository Support Implementation

## Overview
This document describes the implementation of multi-repository support for sessions, allowing a single session to work with multiple repositories.

## Changes Made

### 1. Database Schema
- **New Table**: `session_repository` 
  - Stores multiple repositories per session
  - Fields: `id` (UUID), `session_id` (FK to session), `repo`, `target_branch`, timestamps
  - Foreign key with CASCADE on delete/update
  - Index on `session_id` for performance

### 2. Database Migration
- **File**: `migration/src/m20251115_000001_create_session_repositories_table.rs`
- Creates the junction table with proper constraints
- Registered in `migration/src/lib.rs`

### 3. Entity Layer
- **New Entity**: `src/entities/session_repository.rs`
  - SeaORM entity for session_repository table
  - Relation to session entity
- **Updated**: `src/entities/session.rs`
  - Added `SessionRepository` relation
  - `has_many` relationship to session_repository
- **Updated**: `src/entities/mod.rs`
  - Registered session_repository module

### 4. API Layer (Partial - Maintaining Backward Compatibility)
- **Updated**: `src/handlers/sessions.rs`
  - Added `RepositoryInput` struct for array inputs
  - Added `RepositoryDto` for array outputs
  - Updated `CreateSessionInput` to accept both:
    - `repo` + `target_branch` (legacy single repo)
    - `repositories` array (new multi-repo)
  - Updated `CreateSessionWithPromptInput` similarly
  - Updated `SessionDto` to include optional `repositories` array
  - Kept legacy `repo` and `target_branch` fields for backward compatibility

## Backward Compatibility Strategy

The implementation maintains full backward compatibility:

1. **Legacy single repo input**: Still accepted via `repo` + `target_branch` fields
2. **Legacy single repo output**: Still present in SessionDto
3. **New multi-repo input**: Use `repositories` array
4. **New multi-repo output**: Optional `repositories` array in SessionDto

## TODO: Remaining Implementation Work

### Critical for Functionality:
1. **Update `create_with_prompt` handler logic**:
   - Handle both legacy single repo and new repositories array
   - Create session_repository entries when repositories array provided
   - Store first repo in legacy fields for backward compatibility

2. **Update `read` handler**:
   - Load related session_repositories
   - Populate `repositories` array in response

3. **Update `list` handler**:
   - Eager load session_repositories for all sessions
   - Populate `repositories` arrays

4. **Update outbox_publisher.rs**:
   - Query session_repositories for each session
   - Clone all repositories (not just one)
   - Create separate directories for each repo

### Nice to Have:
5. **Data migration script**:
   - Copy existing `repo`/`target_branch` to session_repository table
   - For existing sessions with populated repo field

6. **Add endpoint**: `POST /sessions/{id}/repositories`
   - Add repositories to existing session

7. **Add endpoint**: `DELETE /sessions/{id}/repositories/{repo_id}`
   - Remove repository from session

## Testing Strategy

1. **Test backward compatibility**:
   - Create session with legacy `repo` + `target_branch`
   - Verify it still works

2. **Test multi-repo**:
   - Create session with `repositories` array
   - Verify all repos are stored
   - Verify all repos are returned in GET

3. **Test outbox processing**:
   - Verify all repos are cloned for multi-repo sessions
   - Verify single repo still works

## Example API Usage

### Legacy (Backward Compatible):
```json
POST /sessions/with-prompt
{
  "repo": "owner/repo",
  "target_branch": "main",
  "messages": {...}
}
```

### New Multi-Repo:
```json
POST /sessions/with-prompt
{
  "repositories": [
    {"repo": "owner/repo1", "target_branch": "main"},
    {"repo": "owner/repo2", "target_branch": "develop"}
  ],
  "messages": {...}
}
```

### Response (includes both for compatibility):
```json
{
  "success": true,
  "sessionId": "...",
  "promptId": "..."
}
```

Get session response includes:
```json
{
  "session": {
    "id": "...",
    "repo": "owner/repo1",  // First repo for backward compat
    "targetBranch": "main",
    "repositories": [       // New field
      {"id": "...", "repo": "owner/repo1", "targetBranch": "main"},
      {"id": "...", "repo": "owner/repo2", "targetBranch": "develop"}
    ],
    ...
  }
}
```

## Database Schema Diagram

```
session (existing)
├── id (PK)
├── repo (nullable, legacy)
├── target_branch (nullable, legacy)
└── ... other fields

session_repository (new)
├── id (PK)
├── session_id (FK -> session.id) CASCADE
├── repo
├── target_branch
├── created_at
└── updated_at
```

## Files Modified

1. `migration/src/m20251115_000001_create_session_repositories_table.rs` (new)
2. `migration/src/lib.rs` (updated)
3. `src/entities/session_repository.rs` (new)
4. `src/entities/session.rs` (updated)
5. `src/entities/mod.rs` (updated)
6. `src/handlers/sessions.rs` (partially updated - needs completion)
7. `src/bg_tasks/outbox_publisher.rs` (needs update)

## Next Steps

To complete the implementation:
1. Finish handler logic to create/read session_repositories
2. Update outbox_publisher to handle multiple repos
3. Run cargo fmt and clippy
4. Test the changes
5. Commit and push
6. Create pull request
