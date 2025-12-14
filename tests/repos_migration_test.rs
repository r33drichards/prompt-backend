/// Integration tests for repos field migration
/// Tests backwards compatibility between old `repo` field and new `repos` field
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde_json::json;

use prompt_backend::entities::session::{self, Entity as Session, Model as SessionModel, RepoConfig, ReposConfig, UiStatus};

/// Helper function to create a test session with old-style repo field
async fn create_session_with_old_format(
    db: &DatabaseConnection,
    repo: &str,
    target_branch: &str,
) -> SessionModel {
    let session_id = uuid::Uuid::new_v4();
    
    let new_session = session::ActiveModel {
        id: Set(session_id),
        sbx_config: Set(None),
        parent: Set(None),
        branch: Set(Some("test-branch".to_string())),
        repo: Set(Some(repo.to_string())),
        target_branch: Set(Some(target_branch.to_string())),
        title: Set(Some("Test Session".to_string())),
        ui_status: Set(UiStatus::Pending),
        user_id: Set("test-user".to_string()),
        ip_return_retry_count: Set(0),
        created_at: sea_orm::NotSet,
        updated_at: sea_orm::NotSet,
        deleted_at: Set(None),
        cancellation_status: Set(None),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(None),
        preserve_sandbox: Set(None),
        repos: Set(None),
    };

    new_session.insert(db).await.expect("Failed to create session")
}

/// Helper function to create a test session with new-style repos field
async fn create_session_with_new_format(
    db: &DatabaseConnection,
    repos: Vec<RepoConfig>,
) -> SessionModel {
    let session_id = uuid::Uuid::new_v4();
    
    let repos_config = ReposConfig { repos };
    let repos_json = serde_json::to_value(&repos_config).expect("Failed to serialize repos");
    
    let new_session = session::ActiveModel {
        id: Set(session_id),
        sbx_config: Set(None),
        parent: Set(None),
        branch: Set(Some("test-branch".to_string())),
        repo: Set(None),
        target_branch: Set(None),
        title: Set(Some("Test Session".to_string())),
        ui_status: Set(UiStatus::Pending),
        user_id: Set("test-user".to_string()),
        ip_return_retry_count: Set(0),
        created_at: sea_orm::NotSet,
        updated_at: sea_orm::NotSet,
        deleted_at: Set(None),
        cancellation_status: Set(None),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(None),
        preserve_sandbox: Set(None),
        repos: Set(Some(repos_json)),
    };

    new_session.insert(db).await.expect("Failed to create session")
}

#[tokio::test]
async fn test_old_format_get_repo_url() {
    let db = setup_test_db().await;
    
    let session = create_session_with_old_format(&db, "owner/repo", "main").await;
    
    // Test that get_repo_url returns the old repo field value
    assert_eq!(session.get_repo_url(), Some("owner/repo".to_string()));
    
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_old_format_get_repo_branch() {
    let db = setup_test_db().await;
    
    let session = create_session_with_old_format(&db, "owner/repo", "develop").await;
    
    // Test that get_repo_branch returns the old target_branch field value
    assert_eq!(session.get_repo_branch(), Some("develop".to_string()));
    
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_old_format_get_all_repos() {
    let db = setup_test_db().await;
    
    let session = create_session_with_old_format(&db, "owner/repo", "main").await;
    
    // Test that get_all_repos converts old format to single repo
    let repos = session.get_all_repos();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].url, "owner/repo");
    assert_eq!(repos[0].branch, "main");
    
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_new_format_get_repo_url() {
    let db = setup_test_db().await;
    
    let repos = vec![
        RepoConfig {
            url: "owner/repo1".to_string(),
            branch: "main".to_string(),
        },
        RepoConfig {
            url: "owner/repo2".to_string(),
            branch: "develop".to_string(),
        },
    ];
    
    let session = create_session_with_new_format(&db, repos).await;
    
    // Test that get_repo_url returns the first repo URL
    assert_eq!(session.get_repo_url(), Some("owner/repo1".to_string()));
    
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_new_format_get_repo_branch() {
    let db = setup_test_db().await;
    
    let repos = vec![
        RepoConfig {
            url: "owner/repo1".to_string(),
            branch: "feature-branch".to_string(),
        },
    ];
    
    let session = create_session_with_new_format(&db, repos).await;
    
    // Test that get_repo_branch returns the first repo branch
    assert_eq!(session.get_repo_branch(), Some("feature-branch".to_string()));
    
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_new_format_get_all_repos() {
    let db = setup_test_db().await;
    
    let repos = vec![
        RepoConfig {
            url: "owner/repo1".to_string(),
            branch: "main".to_string(),
        },
        RepoConfig {
            url: "owner/repo2".to_string(),
            branch: "develop".to_string(),
        },
        RepoConfig {
            url: "owner/repo3".to_string(),
            branch: "feature".to_string(),
        },
    ];
    
    let session = create_session_with_new_format(&db, repos.clone()).await;
    
    // Test that get_all_repos returns all repos
    let fetched_repos = session.get_all_repos();
    assert_eq!(fetched_repos.len(), 3);
    assert_eq!(fetched_repos[0].url, "owner/repo1");
    assert_eq!(fetched_repos[0].branch, "main");
    assert_eq!(fetched_repos[1].url, "owner/repo2");
    assert_eq!(fetched_repos[1].branch, "develop");
    assert_eq!(fetched_repos[2].url, "owner/repo3");
    assert_eq!(fetched_repos[2].branch, "feature");
    
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_empty_session_returns_none() {
    let db = setup_test_db().await;
    let session_id = uuid::Uuid::new_v4();
    
    let new_session = session::ActiveModel {
        id: Set(session_id),
        sbx_config: Set(None),
        parent: Set(None),
        branch: Set(Some("test-branch".to_string())),
        repo: Set(None),
        target_branch: Set(None),
        title: Set(Some("Test Session".to_string())),
        ui_status: Set(UiStatus::Pending),
        user_id: Set("test-user".to_string()),
        ip_return_retry_count: Set(0),
        created_at: sea_orm::NotSet,
        updated_at: sea_orm::NotSet,
        deleted_at: Set(None),
        cancellation_status: Set(None),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(None),
        preserve_sandbox: Set(None),
        repos: Set(None),
    };

    let session = new_session.insert(&db).await.expect("Failed to create session");
    
    // Test that methods return None/empty when no repo data is present
    assert_eq!(session.get_repo_url(), None);
    assert_eq!(session.get_repo_branch(), None);
    assert_eq!(session.get_all_repos().len(), 0);
    
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_new_format_takes_precedence() {
    let db = setup_test_db().await;
    let session_id = uuid::Uuid::new_v4();
    
    // Create a session with BOTH old and new format
    // New format should take precedence
    let repos_config = ReposConfig {
        repos: vec![RepoConfig {
            url: "owner/new-repo".to_string(),
            branch: "new-branch".to_string(),
        }],
    };
    let repos_json = serde_json::to_value(&repos_config).expect("Failed to serialize repos");
    
    let new_session = session::ActiveModel {
        id: Set(session_id),
        sbx_config: Set(None),
        parent: Set(None),
        branch: Set(Some("test-branch".to_string())),
        repo: Set(Some("owner/old-repo".to_string())),
        target_branch: Set(Some("old-branch".to_string())),
        title: Set(Some("Test Session".to_string())),
        ui_status: Set(UiStatus::Pending),
        user_id: Set("test-user".to_string()),
        ip_return_retry_count: Set(0),
        created_at: sea_orm::NotSet,
        updated_at: sea_orm::NotSet,
        deleted_at: Set(None),
        cancellation_status: Set(None),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(None),
        preserve_sandbox: Set(None),
        repos: Set(Some(repos_json)),
    };

    let session = new_session.insert(&db).await.expect("Failed to create session");
    
    // New format should take precedence
    assert_eq!(session.get_repo_url(), Some("owner/new-repo".to_string()));
    assert_eq!(session.get_repo_branch(), Some("new-branch".to_string()));
    
    let repos = session.get_all_repos();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].url, "owner/new-repo");
    assert_eq!(repos[0].branch, "new-branch");
    
    cleanup_session(&db, session.id).await;
}

// Helper functions

async fn setup_test_db() -> DatabaseConnection {
    use sea_orm::Database;
    
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://promptuser:promptpass@localhost:5432/prompt_backend".to_string());
    
    Database::connect(&database_url)
        .await
        .expect("Failed to connect to database")
}

async fn cleanup_session(db: &DatabaseConnection, session_id: uuid::Uuid) {
    let _ = Session::delete_by_id(session_id).exec(db).await;
}
