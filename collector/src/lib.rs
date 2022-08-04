use chrono::{DateTime, NaiveDate, Utc};
pub use database::{Commit, PatchName, QueryLabel};
use serde::Deserialize;
use std::cmp::PartialOrd;
use std::fmt;
use std::process::{self, Command};

pub mod api;
pub mod category;
pub mod etw_parser;
mod read2;
pub mod self_profile;

use process::Stdio;
pub use self_profile::{QueryData, SelfProfile};

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Deserialize)]
pub struct DeltaTime(#[serde(with = "round_float")] pub f64);

/// The bound for finding an artifact
///
/// This can either be the upper or lower bound.
/// In the case of commits or tags this is an exact bound, but for dates
/// it's a best effort (i.e., if the bound is a date but there are no artifacts
/// for that date, we'll find the artifact that most closely matches).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    /// An unverified git commit (in sha form) or a tag of a commit (e.g., "1.53.0")
    Commit(String),
    /// A date in time
    Date(NaiveDate),
    /// No bound
    None,
}

impl Bound {
    /// Tests whether `self` matches commit when searching from the left
    pub fn left_match(&self, commit: &Commit) -> bool {
        match self {
            Bound::Commit(sha) => commit.sha == **sha,
            Bound::Date(date) => commit.date.0.naive_utc().date() >= *date,
            Bound::None => {
                let last_month = chrono::Utc::now().date().naive_utc() - chrono::Duration::days(30);
                last_month <= commit.date.0.naive_utc().date()
            }
        }
    }

    /// Tests whether `self` matches commit when searching from the right
    pub fn right_match(&self, commit: &Commit) -> bool {
        match self {
            Bound::Commit(sha) => commit.sha == **sha,
            Bound::Date(date) => commit.date.0.date().naive_utc() <= *date,
            Bound::None => true,
        }
    }
}

impl serde::Serialize for Bound {
    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let s = match *self {
            Bound::Commit(ref s) => s.clone(),
            Bound::Date(ref date) => date.format("%Y-%m-%d").to_string(),
            Bound::None => String::new(),
        };
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for Bound {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Bound, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        struct BoundVisitor;

        impl<'de> serde::de::Visitor<'de> for BoundVisitor {
            type Value = Bound;

            fn visit_str<E>(self, value: &str) -> ::std::result::Result<Bound, E>
            where
                E: serde::de::Error,
            {
                if value.is_empty() {
                    return Ok(Bound::None);
                }

                let bound = value
                    .parse::<chrono::NaiveDate>()
                    .map(|d| Bound::Date(d))
                    .unwrap_or(Bound::Commit(value.to_string()));
                Ok(bound)
            }

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("either a YYYY-mm-dd date or a collection ID (usually commit hash)")
            }
        }

        deserializer.deserialize_str(BoundVisitor)
    }
}

pub fn null_means_nan<'de, D>(deserializer: D) -> ::std::result::Result<f64, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or(0.0))
}

pub fn version_supports_doc(version_str: &str) -> bool {
    if let Some(version) = version_str.parse::<semver::Version>().ok() {
        version >= semver::Version::new(1, 46, 0)
    } else {
        assert!(version_str.starts_with("beta") || version_str.starts_with("master"));
        true
    }
}

pub fn version_supports_incremental(version_str: &str) -> bool {
    if let Some(version) = version_str.parse::<semver::Version>().ok() {
        version >= semver::Version::new(1, 24, 0)
    } else {
        assert!(version_str.starts_with("beta") || version_str.starts_with("master"));
        true
    }
}

/// Rounds serialized and deserialized floats to 2 decimal places.
pub mod round_float {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(n: &f64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64((*n * 100.0).round() / 100.0)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<f64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let n = f64::deserialize(deserializer)?;
        Ok((n * 100.0).round() / 100.0)
    }
}

pub fn run_command(cmd: &mut Command) -> anyhow::Result<()> {
    log::trace!("running: {:?}", cmd);
    let status = cmd.status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("expected success {:?}", status));
    }
    Ok(())
}

#[cfg(windows)]
pub fn robocopy(
    from: &std::path::Path,
    to: &std::path::Path,
    extra_args: &[&dyn AsRef<std::ffi::OsStr>],
) -> anyhow::Result<()> {
    let mut cmd = Command::new("robocopy");
    cmd.arg(from).arg(to).arg("/s").arg("/e");

    for arg in extra_args {
        cmd.arg(arg.as_ref());
    }

    let output = run_command_with_output(&mut cmd)?;

    if output.status.code() >= Some(8) {
        // robocopy returns 0-7 on success
        return Err(anyhow::anyhow!(
            "expected success, got {}\n\nstderr={}\n\n stdout={}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        ));
    }

    Ok(())
}

fn run_command_with_output(cmd: &mut Command) -> anyhow::Result<process::Output> {
    use anyhow::Context;
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn process for cmd: {:?}", cmd))?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_writer = std::io::LineWriter::new(std::io::stdout());
    let mut stderr_writer = std::io::LineWriter::new(std::io::stderr());
    read2::read2(
        child.stdout.take().unwrap(),
        child.stderr.take().unwrap(),
        &mut |is_stdout, buffer, _is_done| {
            // Send output if trace logging is enabled
            if log::log_enabled!(target: "raw_cargo_messages", log::Level::Trace) {
                use std::io::Write;
                if is_stdout {
                    stdout_writer.write_all(&buffer[stdout.len()..]).unwrap();
                } else {
                    stderr_writer.write_all(&buffer[stderr.len()..]).unwrap();
                }
            }
            if is_stdout {
                stdout = buffer.clone();
            } else {
                stderr = buffer.clone();
            }
        },
    )?;

    let status = child
        .wait()
        .with_context(|| "failed to wait on child process")?;

    Ok(process::Output {
        status,
        stdout,
        stderr,
    })
}

pub fn command_output(cmd: &mut Command) -> anyhow::Result<process::Output> {
    let output = run_command_with_output(cmd)?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "expected success, got {}\n\nstderr={}\n\n stdout={}\n",
            output.status,
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        ));
    }

    Ok(output)
}

#[derive(Debug, Clone, Deserialize)]
pub struct MasterCommit {
    pub sha: String,
    pub parent_sha: String,
    /// This is the pull request which this commit merged in.
    #[serde(default)]
    pub pr: Option<u32>,
    pub time: chrono::DateTime<chrono::Utc>,
}

fn extract_pr_number(commit_msg: &str) -> Option<u32> {
    commit_msg
        .strip_prefix("Merge #")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

pub async fn master_commits() -> anyhow::Result<Vec<MasterCommit>> {
    let git_log = Command::new("git")
        .stdout(Stdio::piped())
        .current_dir("../prusti-dev")
        .arg("--no-pager")
        .arg("log")
        .arg("origin/master")
        .arg("--author=bors")
        .arg("--pretty=format:%H,%aI,%s")
        .output()
        .unwrap();

    let git_output = std::str::from_utf8(&git_log.stdout).unwrap();
    let mut output_lines = git_output.lines();

    let mut result: Vec<MasterCommit> = Vec::new();

    let mut cur_line = output_lines.next().unwrap();

    while let Some(parent) = output_lines.next() {
        let mut cur_chunks = cur_line.split(",");
        let sha = cur_chunks.next().unwrap();
        let time = cur_chunks.next().unwrap();
        let commit_msg = cur_chunks.next().unwrap();

        let parent_sha = parent.split(",").next().unwrap();
        result.push(MasterCommit {
            sha: sha.to_string(),
            parent_sha: parent_sha.to_string(),
            pr: extract_pr_number(commit_msg),
            time: DateTime::parse_from_rfc3339(time).unwrap().with_timezone(&Utc)
        });
        cur_line = parent;
    }
    log::info!("{} commits to master", result.len());
    Ok(result)
}
