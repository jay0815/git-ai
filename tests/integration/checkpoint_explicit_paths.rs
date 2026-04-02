use crate::repos::test_repo::TestRepo;
use serde_json::json;
use std::collections::HashMap;
use std::fs;

fn write_base_files(repo: &TestRepo) {
    fs::write(repo.path().join("lines.md"), "base lines\n").expect("failed to write lines.md");
    fs::write(repo.path().join("alphabet.md"), "base alphabet\n")
        .expect("failed to write alphabet.md");
    repo.stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");
}

fn latest_checkpoint_files(repo: &TestRepo) -> Vec<String> {
    let checkpoints = repo
        .current_working_logs()
        .read_all_checkpoints()
        .expect("checkpoints should be readable");
    checkpoints
        .last()
        .expect("latest checkpoint should exist")
        .entries
        .iter()
        .map(|entry| entry.file.clone())
        .collect()
}

#[test]
fn test_explicit_path_checkpoint_only_tracks_the_explicit_file() {
    let repo = TestRepo::new();
    write_base_files(&repo);

    fs::write(
        repo.path().join("lines.md"),
        "line touched by first checkpoint\n",
    )
    .expect("failed to update lines.md");
    repo.git_ai(&["checkpoint", "mock_ai", "lines.md"])
        .expect("first explicit checkpoint should succeed");

    fs::write(
        repo.path().join("alphabet.md"),
        "line touched by second checkpoint\n",
    )
    .expect("failed to update alphabet.md");
    repo.git_ai(&["checkpoint", "mock_ai", "alphabet.md"])
        .expect("second explicit checkpoint should succeed");

    assert_eq!(
        latest_checkpoint_files(&repo),
        vec!["alphabet.md".to_string()],
        "explicit path checkpoints must not expand to other dirty AI-touched files"
    );
}

#[test]
fn test_explicit_path_checkpoint_skips_conflicted_files() {
    let repo = TestRepo::new();
    let conflict_path = repo.path().join("conflict.txt");
    fs::write(&conflict_path, "base\n").expect("failed to write conflict.txt");
    repo.stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    let base_branch = repo.current_branch();

    repo.git_og(&["checkout", "-b", "feature-branch"])
        .expect("feature branch checkout should succeed");
    fs::write(&conflict_path, "feature\n").expect("failed to write feature content");
    repo.git_og(&["add", "conflict.txt"])
        .expect("feature add should succeed");
    repo.git_og(&["commit", "-m", "feature commit"])
        .expect("feature commit should succeed");

    repo.git_og(&["checkout", &base_branch])
        .expect("return to base branch should succeed");
    fs::write(&conflict_path, "main\n").expect("failed to write main content");
    repo.git_og(&["add", "conflict.txt"])
        .expect("main add should succeed");
    repo.git_og(&["commit", "-m", "main commit"])
        .expect("main commit should succeed");

    let merge_result = repo.git_og(&["merge", "feature-branch"]);
    assert!(merge_result.is_err(), "merge should conflict");
    assert!(
        repo.git_og(&["status", "--short"])
            .expect("status should be readable")
            .contains("UU conflict.txt"),
        "merge should leave conflict.txt unmerged"
    );

    repo.git_ai(&["checkpoint", "mock_ai", "conflict.txt"])
        .expect("explicit conflict checkpoint should succeed without recording entries");

    let checkpoints = repo
        .current_working_logs()
        .read_all_checkpoints()
        .expect("checkpoints should be readable");
    assert!(
        checkpoints.is_empty(),
        "explicit-path checkpoints should skip conflicted files entirely"
    );
}

#[test]
fn test_explicit_path_checkpoint_skips_binary_replacements() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("sample.txt");
    fs::write(&file_path, "hello\n").expect("failed to write sample.txt");
    repo.stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    fs::write(&file_path, vec![0x00, 0x01, 0x02, 0xFF, 0xFE])
        .expect("failed to write binary replacement");

    repo.git_ai(&["checkpoint", "mock_ai", "sample.txt"])
        .expect("explicit binary checkpoint should succeed without recording entries");

    let checkpoints = repo
        .current_working_logs()
        .read_all_checkpoints()
        .expect("checkpoints should be readable");
    assert!(
        checkpoints.is_empty(),
        "explicit-path checkpoints should skip files whose current contents are binary"
    );
}

#[test]
fn test_explicit_path_checkpoint_skips_gitignored_worktree_files() {
    let repo = TestRepo::new();
    write_base_files(&repo);

    fs::write(repo.path().join(".gitignore"), "ignored/**\n").expect("failed to write .gitignore");
    fs::create_dir_all(repo.path().join("ignored")).expect("failed to create ignored dir");
    fs::write(
        repo.path().join("alphabet.md"),
        "line touched by explicit checkpoint\n",
    )
    .expect("failed to update alphabet.md");
    fs::write(
        repo.path().join("ignored").join("generated.md"),
        "ignored worktree content\n",
    )
    .expect("failed to write ignored file");

    repo.git_ai(&[
        "checkpoint",
        "mock_ai",
        "alphabet.md",
        "ignored/generated.md",
    ])
    .expect("explicit checkpoint should succeed");

    assert_eq!(
        latest_checkpoint_files(&repo),
        vec!["alphabet.md".to_string()],
        "explicit-path checkpoints should skip .gitignore'd worktree files"
    );
}

#[test]
fn test_explicit_path_checkpoint_skips_gitignored_dirty_snapshot_files() {
    let repo = TestRepo::new();
    write_base_files(&repo);

    fs::write(repo.path().join(".gitignore"), "ignored/**\n").expect("failed to write .gitignore");
    fs::create_dir_all(repo.path().join("ignored")).expect("failed to create ignored dir");

    let tracked_path = repo.path().join("alphabet.md");
    let ignored_path = repo.path().join("ignored").join("generated.md");
    let tracked_path_str = tracked_path.to_string_lossy().to_string();
    let ignored_path_str = ignored_path.to_string_lossy().to_string();

    fs::write(&ignored_path, "ignored worktree content\n").expect("failed to write ignored file");

    let hook_input = json!({
        "type": "ai_agent",
        "repo_working_dir": repo.path(),
        "edited_filepaths": [tracked_path_str.clone(), ignored_path_str.clone()],
        "transcript": {"messages": []},
        "agent_name": "test-agent",
        "model": "test-model",
        "conversation_id": "test-123",
        "dirty_files": HashMap::from([
            (tracked_path_str, "alphabet snapshot content\n".to_string()),
            (ignored_path_str, "ignored snapshot content\n".to_string()),
        ]),
    });

    repo.git_ai(&[
        "checkpoint",
        "agent-v1",
        "--hook-input",
        &hook_input.to_string(),
    ])
    .expect("explicit dirty snapshot checkpoint should succeed");

    assert_eq!(
        latest_checkpoint_files(&repo),
        vec!["alphabet.md".to_string()],
        "explicit-path checkpoints should skip .gitignore'd dirty snapshot files"
    );
}
