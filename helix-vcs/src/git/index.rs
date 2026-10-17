//! The index as a diff base, and staging, which writes the index through the
//! `git` binary. For the review stream.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use gix::bstr::{BString, ByteSlice};
use gix::filter::plumbing::driver::apply::Delay;
use gix::index::entry::{Flags, Mode, Stage};
use gix::Repository;

use super::{get_repo_dir, open_repo};

/// The repository that holds a file, and the file's path in its index.
struct Location {
    repo: Repository,
    work_dir: PathBuf,
    rela_path: BString,
}

fn locate(file: &Path, trust_full: bool) -> Result<Location> {
    let file = gix::path::realpath(file).context("resolve symlinks")?;
    let repo = open_repo(get_repo_dir(&file)?, trust_full)
        .context("failed to open git repo")?
        .to_thread_local();
    let work_dir = repo
        .workdir()
        .context("repo has no worktree")?
        .to_path_buf();
    let rela_path = gix::path::try_into_bstr(file.strip_prefix(&work_dir)?)?;
    let rela_path = gix::path::to_unix_separators_on_windows(rela_path).into_owned();
    Ok(Location {
        repo,
        work_dir,
        rela_path,
    })
}

/// The index version of `file`, as worktree text like the `HEAD` base.
/// `None` when the index has no unconflicted entry for it.
pub fn get_index_base(file: &Path, trust_full: bool) -> Result<Option<Vec<u8>>> {
    let Location {
        repo, rela_path, ..
    } = locate(file, trust_full)?;
    let index = repo.index_or_empty()?;
    let Some(entry) = index.entry_by_path_and_stage(rela_path.as_bstr(), Stage::Unconflicted)
    else {
        return Ok(None);
    };
    // WHY: `git add --intent-to-add` records an empty file.
    if entry.flags.contains(Flags::INTENT_TO_ADD) {
        return Ok(Some(Vec::new()));
    }
    if entry.mode != Mode::FILE && entry.mode != Mode::FILE_EXECUTABLE {
        bail!("{} is not a file in the index", file.display());
    }

    let data = repo.find_object(entry.id)?.detach().data;
    let (mut pipeline, _) = repo.filter_pipeline(None)?;
    let mut worktree_outcome =
        pipeline.convert_to_worktree(&data, rela_path.as_bstr(), Delay::Forbid)?;
    let mut buf = Vec::with_capacity(data.len());
    worktree_outcome.read_to_end(&mut buf)?;
    Ok(Some(buf))
}

/// Makes `text`, given as worktree text, the index version of `file`, or
/// removes `file` from the index when `text` is `None`.
///
/// `git hash-object --path` converts the text the way `git add` would, with
/// CRLF conversion and clean filters. `git` runs programs that repository-local
/// config names, like clean filters and `core.fsmonitor`, so this refuses an
/// untrusted workspace.
pub fn set_index_text(file: &Path, text: Option<&[u8]>, trust_full: bool) -> Result<()> {
    if !trust_full {
        bail!("staging runs git, which needs a trusted workspace");
    }
    let Location {
        repo,
        work_dir,
        rela_path,
    } = locate(file, trust_full)?;
    let index = repo.index_or_empty()?;
    let entries = index
        .entry_range(rela_path.as_bstr())
        .map_or(&[][..], |range| &index.entries()[range]);
    if entries
        .iter()
        .any(|entry| entry.stage() != Stage::Unconflicted)
    {
        bail!("{} has unresolved conflicts", file.display());
    }
    let path = gix::path::from_bstr(rela_path.as_bstr());
    let git = |args: &[&OsStr], input: &[u8]| run_git(&repo, &work_dir, args, input);

    let Some(text) = text else {
        git(
            &[
                "update-index".as_ref(),
                "--force-remove".as_ref(),
                "--".as_ref(),
                path.as_os_str(),
            ],
            &[],
        )?;
        return Ok(());
    };
    let mode = match entries.first() {
        Some(entry) if entry.mode == Mode::FILE => "100644",
        Some(entry) if entry.mode == Mode::FILE_EXECUTABLE => "100755",
        Some(_) => bail!("{} is not a file in the index", file.display()),
        None if is_executable(&repo, file) => "100755",
        None => "100644",
    };
    let oid = git(
        &[
            "hash-object".as_ref(),
            "-w".as_ref(),
            "--stdin".as_ref(),
            "--path".as_ref(),
            path.as_os_str(),
        ],
        text,
    )?;
    let oid = oid.trim().to_os_str()?;
    git(
        &[
            "update-index".as_ref(),
            "--add".as_ref(),
            "--cacheinfo".as_ref(),
            mode.as_ref(),
            oid,
            path.as_os_str(),
        ],
        &[],
    )?;
    Ok(())
}

/// Runs `git` on `repo`, feeds it `input`, and returns what it printed.
fn run_git(repo: &Repository, work_dir: &Path, args: &[&OsStr], input: &[u8]) -> Result<Vec<u8>> {
    let mut child = Command::new("git")
        .current_dir(work_dir)
        // WHY: the repository gix found, even if `GIT_DIR` names another one.
        .arg("--git-dir")
        .arg(repo.git_dir())
        .arg("--work-tree")
        .arg(work_dir)
        .args(args)
        // INVARIANT: gix reads `<git-dir>/index`, so `git` must write that one.
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to run git")?;
    let mut stdin = child.stdin.take().context("git has no stdin")?;
    // WHY: write on another thread, so a full stdout or stderr pipe can't
    // block both sides.
    let output = std::thread::scope(|scope| {
        scope.spawn(move || {
            // WHY: a failed write shows in git's exit status.
            let _ = stdin.write_all(input);
        });
        child.wait_with_output()
    })?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args[0].to_string_lossy(),
            output.stderr.trim().to_str_lossy()
        );
    }
    Ok(output.stdout)
}

/// Whether `git add` would record a new `file` as executable.
#[cfg(unix)]
fn is_executable(repo: &Repository, file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    repo.config_snapshot()
        .boolean("core.fileMode")
        .unwrap_or(true)
        && file
            .metadata()
            .is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_repo: &Repository, _file: &Path) -> bool {
    false
}

#[cfg(test)]
mod test {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::{get_index_base, set_index_text};

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env("GIT_AUTHOR_EMAIL", "author@example.com")
            .env("GIT_AUTHOR_NAME", "author")
            .env("GIT_COMMITTER_EMAIL", "committer@example.com")
            .env("GIT_COMMITTER_NAME", "committer")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
            .env("GIT_CONFIG_VALUE_0", "false")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "`git {args:?}` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// A repository with `files` committed.
    fn repo(files: &[(&str, &str)]) -> TempDir {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "--quiet"]);
        for (name, text) in files {
            std::fs::write(repo.path().join(name), text).unwrap();
        }
        git(repo.path(), &["add", "--all"]);
        git(repo.path(), &["commit", "--quiet", "--message", "base"]);
        repo
    }

    fn index_text(repo: &TempDir, name: &str) -> String {
        git(repo.path(), &["show", &format!(":{name}")])
    }

    #[test]
    fn the_index_base_is_the_staged_text() {
        let repo = repo(&[("a.txt", "a1\na2\n")]);
        let file = repo.path().join("a.txt");
        std::fs::write(&file, "A1\na2\n").unwrap();
        git(repo.path(), &["add", "a.txt"]);
        std::fs::write(&file, "A1\nA2\n").unwrap();

        assert_eq!(
            get_index_base(&file, false).unwrap().as_deref(),
            Some(&b"A1\na2\n"[..])
        );
    }

    #[test]
    fn an_untracked_file_has_no_index_base() {
        let repo = repo(&[("a.txt", "a1\n")]);
        let file = repo.path().join("new.txt");
        std::fs::write(&file, "new\n").unwrap();

        assert_eq!(get_index_base(&file, false).unwrap(), None);
    }

    #[test]
    fn staging_converts_worktree_text_like_git_add() {
        let repo = repo(&[
            (".gitattributes", "*.txt text eol=crlf\n"),
            ("a.txt", "a\nb\n"),
        ]);
        let file = repo.path().join("a.txt");

        // WHY: the base is worktree text, so it has the CRLF line endings.
        assert_eq!(
            get_index_base(&file, false).unwrap().as_deref(),
            Some(&b"a\r\nb\r\n"[..])
        );
        set_index_text(&file, Some(b"a\r\nB\r\n"), true).unwrap();
        assert_eq!(index_text(&repo, "a.txt"), "a\nB\n");
    }

    #[test]
    fn staging_adds_and_removes_files() {
        let repo = repo(&[("a.txt", "a\n")]);
        let new = repo.path().join("new.txt");
        std::fs::write(&new, "new\n").unwrap();

        set_index_text(&new, Some(b"new\n"), true).unwrap();
        assert_eq!(index_text(&repo, "new.txt"), "new\n");
        set_index_text(&repo.path().join("a.txt"), None, true).unwrap();
        assert_eq!(git(repo.path(), &["ls-files", "--cached"]), "new.txt\n");
    }

    #[test]
    fn staging_keeps_the_executable_bit() {
        let repo = repo(&[("run.sh", "echo\n")]);
        git(repo.path(), &["update-index", "--chmod=+x", "run.sh"]);

        set_index_text(&repo.path().join("run.sh"), Some(b"echo hi\n"), true).unwrap();
        assert!(git(repo.path(), &["ls-files", "--stage", "run.sh"]).starts_with("100755 "));
    }

    #[test]
    fn staging_needs_a_trusted_workspace() {
        let repo = repo(&[("a.txt", "a\n")]);
        let file = repo.path().join("a.txt");

        let err = set_index_text(&file, Some(b"A\n"), false).unwrap_err();
        assert_eq!(
            err.to_string(),
            "staging runs git, which needs a trusted workspace"
        );
        assert_eq!(index_text(&repo, "a.txt"), "a\n");
    }
}
