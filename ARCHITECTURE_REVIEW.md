# Architecture Review: IP Return Job Queue Implementation

## Current Implementation Analysis

### Flow Overview
```
OutboxJob (Apalis)
  └─> process_outbox_job()
      ├─> Setup git repo, Claude CLI
      ├─> tokio::spawn (fire-and-forget) ──┐
      └─> Returns Ok(()) immediately        │
                                            │
                                            ▼
                    Spawned Task (not tracked by Apalis):
                      ├─> Run Claude CLI (hours)
                      ├─> Update session → ReturningIp
                      └─> Enqueue IpReturnJob
                                            │
                                            ▼
                    IpReturnJob (Apalis)
                      └─> process_ip_return_job()
                          ├─> Return IP to allocator
                          ├─> Set sbx_config = null
                          └─> Update session → Archived
```

## ✅ Strengths

1. **IP Return is Retryable**: Using Apalis for IP return provides automatic retries
2. **Observability**: Session status clearly indicates state
3. **Decoupled**: IP return failures don't block new jobs
4. **Database-backed**: Job queue survives restarts

## ⚠️ Critical Issues

### Issue #1: Fire-and-Forget Spawned Task

**Problem:**
```rust
tokio::spawn(async move {
    // Run Claude CLI for hours...
});
Ok(())  // <-- Apalis thinks job is done!
```

**Consequences:**
- ❌ No retry if spawned task panics
- ❌ No visibility into running Claude sessions
- ❌ If process restarts, running sessions are orphaned
- ❌ IP return job never enqueued if task crashes
- ❌ Metrics show "completed" but work still running

### Issue #2: State Transition Race Conditions

**Problem:**
```rust
// If this fails, session stuck in ReturningIp forever
match storage.push(ip_return_job).await {
    Err(e) => {
        error!("Failed to enqueue IP return job...");
        // Session is now in ReturningIp but no job exists!
    }
}
```

### Issue #3: No Idempotency Guarantees

**Problem:**
- What if IP return job runs twice?
- What if IP was already returned?
- No checks for duplicate returns

## 🎯 Best Practice Architectures

### Option A: Poller-Based IP Return (RECOMMENDED)

Similar to how `prompt_poller` works - most resilient approach.

**New file:** `src/bg_tasks/ip_return_poller.rs`
```rust
/// Periodic poller that checks for sessions in ReturningIp status
pub async fn run_ip_return_poller(db: DatabaseConnection) -> anyhow::Result<()> {
    info!("Starting IP return poller - checking every 5 seconds");

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Find all sessions in ReturningIp status
        let sessions = Session::find()
            .filter(session::Column::SessionStatus.eq(SessionStatus::ReturningIp))
            .filter(session::Column::SbxConfig.is_not_null())
            .all(&db)
            .await?;

        for session in sessions {
            match process_ip_return(&db, session).await {
                Ok(_) => info!("Returned IP for session {}", session.id),
                Err(e) => error!("Failed to return IP: {}", e),
                // Will retry on next poll cycle
            }
        }
    }
}

async fn process_ip_return(db: &DatabaseConnection, session: session::Model) -> anyhow::Result<()> {
    let ip_allocator_url = std::env::var("IP_ALLOCATOR_URL")?;
    let ip_client = ip_allocator_client::Client::new(&ip_allocator_url);

    // Return the IP
    if let Some(sbx_config) = &session.sbx_config {
        ip_client.handlers_ip_return_item(&ReturnInput {
            item: sbx_config.clone()
        }).await?;
    }

    // Update session (idempotent - can run multiple times safely)
    let mut active_session: session::ActiveModel = session.into();
    active_session.sbx_config = Set(None);
    active_session.session_status = Set(SessionStatus::Archived);
    active_session.status_message = Set(Some("IP returned successfully".to_string()));
    active_session.update(db).await?;

    Ok(())
}
```

**Advantages:**
- ✅ Self-healing: Automatically picks up orphaned sessions
- ✅ Simple: No job queue complexity for IP return
- ✅ Resilient: Survives process restarts
- ✅ Idempotent: Safe to run multiple times
- ✅ No race conditions between status update and job enqueue

**Implementation:**
1. Remove `IpReturnJob` and `ip_returner` worker
2. Add `ip_return_poller` like `prompt_poller`
3. Outbox publisher just updates status to `ReturningIp`
4. Poller handles the actual return

---

### Option B: Keep Current but Add Safeguards

If you prefer the job-based approach, add these improvements:

```rust
// In outbox_publisher.rs
tokio::spawn(async move {
    // Run Claude CLI...

    // Wrap in retry logic
    for attempt in 0..3 {
        match enqueue_ip_return(&db_clone_for_return, &pool_clone, session_id).await {
            Ok(_) => break,
            Err(e) => {
                error!("Attempt {} to enqueue IP return failed: {}", attempt, e);
                tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await;
            }
        }
    }
});

async fn enqueue_ip_return(
    db: &DatabaseConnection,
    pool: &PgPool,
    session_id: Uuid,
) -> anyhow::Result<()> {
    // Atomic: update status AND enqueue in transaction
    let mut storage = PostgresStorage::new(pool.clone());

    // First enqueue the job
    storage.push(IpReturnJob {
        session_id: session_id.to_string(),
    }).await?;

    // Then update status (if this fails, job will still run)
    let session = Session::find_by_id(session_id).one(db).await?
        .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

    let mut active_session: session::ActiveModel = session.into();
    active_session.session_status = Set(SessionStatus::ReturningIp);
    active_session.update(db).await?;

    Ok(())
}
```

**Add idempotency to ip_returner.rs:**
```rust
pub async fn process_ip_return_job(job: IpReturnJob, ctx: Data<IpReturnContext>) -> Result<(), Error> {
    // ... parse session_id ...

    let session_model = Session::find_by_id(session_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| Error::Failed("Session not found".into()))?;

    // Idempotency: If already archived with no sbx_config, job is done
    if session_model.session_status == SessionStatus::Archived
        && session_model.sbx_config.is_none()
    {
        info!("IP already returned for session {}, skipping", session_id);
        return Ok(());
    }

    // Only return if sbx_config exists
    if let Some(borrowed_ip_json) = session_model.sbx_config.as_ref() {
        // ... return IP ...
    }

    // Update atomically...
}
```

---

### Option C: Hybrid - Job Queue + Safety Poller

Combine both approaches for maximum resilience:

1. **Primary path:** Job-based (current implementation)
2. **Backup path:** Poller checks for stuck sessions every 60 seconds

```rust
// Safety poller runs periodically
pub async fn run_ip_return_safety_poller(db: DatabaseConnection, pool: PgPool) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;

        // Find sessions stuck in ReturningIp for > 5 minutes
        let five_mins_ago = Utc::now() - Duration::from_secs(300);
        let stuck_sessions = Session::find()
            .filter(session::Column::SessionStatus.eq(SessionStatus::ReturningIp))
            .filter(session::Column::UpdatedAt.lt(five_mins_ago))
            .filter(session::Column::SbxConfig.is_not_null())
            .all(&db)
            .await?;

        if !stuck_sessions.is_empty() {
            warn!("Found {} stuck sessions in ReturningIp", stuck_sessions.len());

            // Re-enqueue jobs for stuck sessions
            for session in stuck_sessions {
                enqueue_ip_return_job(&pool, session.id).await?;
            }
        }
    }
}
```

## 📊 Comparison Matrix

| Approach | Complexity | Resilience | Observability | Idempotency | Restartability |
|----------|------------|------------|---------------|-------------|----------------|
| **Current** | Medium | ⚠️ Poor | Good | ❌ No | ❌ No |
| **Option A (Poller)** | Low | ✅ Excellent | Good | ✅ Yes | ✅ Yes |
| **Option B (Improved Job)** | Medium | ⚠️ Fair | Excellent | ✅ Yes | ⚠️ Partial |
| **Option C (Hybrid)** | High | ✅ Excellent | Excellent | ✅ Yes | ✅ Yes |

## 🎯 Recommendation

**Use Option A (Poller-Based)** because:

1. ✅ Simpler than job-based approach
2. ✅ Self-healing by design
3. ✅ Follows existing pattern (prompt_poller)
4. ✅ No race conditions
5. ✅ Naturally idempotent

The job queue adds complexity without providing value here, since:
- IP return is lightweight (single HTTP call)
- Doesn't need complex retry logic (poller retries automatically)
- Status in database already provides queue semantics

## 🔧 Implementation Steps for Option A

1. Create `src/bg_tasks/ip_return_poller.rs`
2. Add constant `pub const IP_RETURN_POLLER: &str = "ip-return-poller";`
3. Register in `TaskContext` (similar to prompt_poller)
4. Delete `src/bg_tasks/ip_returner.rs`
5. Remove `IP_RETURNER` from mod.rs
6. Simplify outbox_publisher to just update status

Would you like me to implement Option A?
