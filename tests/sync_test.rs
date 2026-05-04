use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use rupo::manifest::toml::{Defaults, Manifest, ProjectEntry, RemoteEntry};
use rupo::sync::parallel::{self, SyncOptions};

/// Create a bare git repository with a single committed file.
fn create_bare_repo(parent: &Path, name: &str) -> String {
    let repo_dir = parent.join(format!("{name}.git"));
    let work_dir = parent.join(format!("{name}-work"));

    Command::new("git")
        .args(["init", "--initial-branch=main"])
        .arg(&work_dir)
        .output()
        .unwrap();
    std::fs::write(work_dir.join("README.md"), format!("# {name}\n")).unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&work_dir)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@test.com",
            "commit",
            "-m",
            "init",
        ])
        .current_dir(&work_dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["clone", "--bare"])
        .arg(&work_dir)
        .arg(&repo_dir)
        .output()
        .unwrap();

    repo_dir.to_string_lossy().into_owned()
}

#[tokio::test]
async fn sync_with_empty_project_list_succeeds() {
    let tmp = TempDir::new().unwrap();

    let manifest = Manifest {
        defaults: None,
        remotes: vec![],
        projects: vec![],
    };

    let opts = SyncOptions {
        jobs: 2,
        current_branch: false,
        depth: None,
    };

    let result = parallel::run(tmp.path(), &manifest, &opts).await;

    assert!(result.success.is_empty());
    assert!(result.failure.is_empty());
}

#[tokio::test]
async fn sync_result_reports_all_projects() {
    let tmp = TempDir::new().unwrap();

    // Create one real bare repo for "core"
    let repos_dir = tmp.path().join("repos");
    std::fs::create_dir(&repos_dir).unwrap();
    create_bare_repo(&repos_dir, "core");

    // The fetch URL is the parent directory; project name is appended as "core.git"
    // But our URL construction is: fetch + "/" + project.name
    // So we set fetch = repos_dir path, name = "core.git" (matches bare repo dirname)
    let fetch_base = repos_dir.to_string_lossy().into_owned();

    let manifest = Manifest {
        defaults: Some(Defaults {
            revision: Some("main".to_string()),
            remote: Some("origin".to_string()),
        }),
        remotes: vec![RemoteEntry {
            name: "origin".to_string(),
            fetch: fetch_base,
        }],
        projects: vec![
            ProjectEntry {
                path: "app/core".to_string(),
                name: "core.git".to_string(),
                revision: Some("main".to_string()),
                remote: None,
                groups: vec![],
                copyfiles: vec![],
                linkfiles: vec![],
            },
            ProjectEntry {
                path: "app/broken".to_string(),
                name: "nonexistent-repo.git".to_string(),
                revision: None,
                remote: None,
                groups: vec![],
                copyfiles: vec![],
                linkfiles: vec![],
            },
        ],
    };

    let work_dir = tmp.path().join("project");
    std::fs::create_dir(&work_dir).unwrap();

    let opts = SyncOptions {
        jobs: 2,
        current_branch: false,
        depth: None,
    };

    let result = parallel::run(&work_dir, &manifest, &opts).await;

    // "core" should succeed
    assert!(
        result.success.contains(&"app/core".to_string()),
        "expected app/core in success, got: {:?}",
        result.success
    );

    // "broken" should fail
    let failed_paths: Vec<&str> = result.failure.iter().map(|(p, _)| p.as_str()).collect();
    assert!(
        failed_paths.contains(&"app/broken"),
        "expected app/broken in failure, got: {:?}",
        result.failure
    );

    // Total = 2 projects
    assert_eq!(result.success.len() + result.failure.len(), 2);
}
