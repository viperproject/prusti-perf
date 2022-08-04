use anyhow::Context;
use http::header::USER_AGENT;

use crate::{api::github::Issue, load::SiteCtxt};

/// A client for interacting with the GitHub API
pub struct Client {
    repository_url: String,
    token: String,
    inner: reqwest::Client,
}

impl Client {
    /// Create a new client from a URL and token (useful for testing)
    pub fn new(repository_url: String, token: String) -> Self {
        Self {
            repository_url,
            token,
            inner: reqwest::Client::new(),
        }
    }

    /// Create a client from a `SiteCtxt` and a URL
    pub fn from_ctxt(ctxt: &SiteCtxt, repository_url: String) -> Self {
        let token = ctxt
            .config
            .keys
            .github_api_token
            .clone()
            .expect("needs github API token");
        Self::new(repository_url, token)
    }

    pub async fn create_ref(&self, ref_: &str, sha: &str) -> anyhow::Result<()> {
        #[derive(serde::Serialize)]
        struct CreateRefRequest<'a> {
            // Must start with `refs/` and have at least two slashes.
            // e.g. `refs/heads/master`.
            #[serde(rename = "ref")]
            ref_: &'a str,
            sha: &'a str,
        }
        let url = format!("{}/git/refs", self.repository_url);
        let req = self.inner.post(&url).json(&CreateRefRequest { ref_, sha });
        let response = self.send(req).await.context("POST git/refs failed")?;
        if response.status() != reqwest::StatusCode::CREATED {
            anyhow::bail!("{:?} != 201 CREATED", response.status());
        }

        Ok(())
    }

    pub async fn create_pr(
        &self,
        title: &str,
        head: &str,
        base: &str,
        description: &str,
        draft: bool,
    ) -> anyhow::Result<CreatePrResponse> {
        #[derive(serde::Serialize)]
        struct CreatePrRequest<'a> {
            title: &'a str,
            // username:branch if cross-repo
            head: &'a str,
            // branch to pull into (e.g, master)
            base: &'a str,
            #[serde(rename = "body")]
            description: &'a str,
            draft: bool,
        }

        let url = format!("{}/pulls", self.repository_url);
        let req = self.inner.post(&url).json(&CreatePrRequest {
            title,
            head,
            base,
            description,
            draft,
        });
        let response = self.send(req).await.context("POST pulls failed")?;
        if response.status() != reqwest::StatusCode::CREATED {
            anyhow::bail!("{:?} != 201 CREATED", response.status());
        }

        Ok(response.json().await.context("deserializing failed")?)
    }

    pub async fn update_branch(&self, branch: &str, sha: &str) -> anyhow::Result<()> {
        #[derive(serde::Serialize)]
        struct UpdateBranchRequest<'a> {
            sha: &'a str,
            force: bool,
        }
        let url = format!("{}/git/refs/heads/{}", self.repository_url, branch);
        let req = self
            .inner
            .patch(&url)
            .json(&UpdateBranchRequest { sha, force: true });

        let response = self.send(req).await.context("PATCH git/refs failed")?;
        if response.status() != reqwest::StatusCode::OK {
            anyhow::bail!("{:?} != 200 OK", response.status());
        }

        Ok(())
    }

    pub async fn merge_branch(
        &self,
        branch: &str,
        sha: &str,
        commit_message: &str,
    ) -> anyhow::Result<String> {
        #[derive(serde::Serialize)]
        struct MergeBranchRequest<'a> {
            base: &'a str,
            head: &'a str,
            commit_message: &'a str,
        }
        let url = format!("{}/merges", self.repository_url);
        let req = self.inner.post(&url).json(&MergeBranchRequest {
            base: branch,
            head: sha,
            commit_message,
        });
        let response = self.send(req).await.context("PATCH /merges failed")?;
        if !response.status().is_success() {
            anyhow::bail!("{:?} != 201 CREATED", response.status());
        }

        Ok(response.json::<MergeBranchResponse>().await?.sha)
    }

    pub async fn create_commit(
        &self,
        message: &str,
        tree: &str,
        parents: &[&str],
    ) -> anyhow::Result<String> {
        #[derive(serde::Serialize)]
        struct CreateCommitRequest<'a> {
            message: &'a str,
            tree: &'a str,
            parents: &'a [&'a str],
        }
        let url = format!("{}/git/commits", self.repository_url);
        let req = self.inner.post(&url).json(&CreateCommitRequest {
            message,
            tree,
            parents,
        });

        let response = self.send(req).await.context("POST git/commits failed")?;
        if response.status() != reqwest::StatusCode::CREATED {
            anyhow::bail!("{:?} != 201 CREATED", response.status());
        }

        Ok(response
            .json::<CreateCommitResponse>()
            .await
            .context("deserializing failed")?
            .sha)
    }

    pub async fn get_issue(&self, number: u64) -> anyhow::Result<Issue> {
        let url = format!("{}/issues/{}", self.repository_url, number);
        let req = self.inner.get(&url);
        let response = self.send(req).await.context("cannot get issue")?;
        if !response.status().is_success() {
            anyhow::bail!("{:?} != 200 OK", response.status());
        }

        Ok(response.json().await?)
    }

    pub async fn get_commit(&self, sha: &str) -> anyhow::Result<Commit> {
        let url = format!("{}/commits/{}", self.repository_url, sha);
        let req = self.inner.get(&url);
        let response = self.send(req).await.context("cannot get commit")?;
        if !response.status().is_success() {
            anyhow::bail!("{:?} != 200 OK", response.status());
        }
        response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("cannot deserialize commit: {:?}", e))
    }

    pub async fn post_comment<B>(&self, pr_number: u32, body: B)
    where
        B: Into<String>,
    {
        #[derive(Debug, Clone, serde::Serialize)]
        pub struct PostComment {
            pub body: String,
        }
        let body = body.into();
        let req = self
            .inner
            .post(&format!(
                "{}/issues/{}/comments",
                self.repository_url, pr_number
            ))
            .json(&PostComment {
                body: body.to_owned(),
            });
        let resp = self.send(req).await;

        if let Err(e) = resp {
            eprintln!("failed to post comment: {:?}", e);
        }
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        request
            .header(USER_AGENT, "perf-rust-lang-org-server")
            .basic_auth("rust-timer", Some(&self.token))
            .send()
            .await
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct CreatePrResponse {
    pub number: u32,
    pub html_url: String,
    pub comments_url: String,
}

#[derive(serde::Deserialize)]
struct MergeBranchResponse {
    sha: String,
}

#[derive(serde::Deserialize)]
struct CreateCommitResponse {
    sha: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Commit {
    pub sha: String,
    pub commit: InnerCommit,
    pub parents: Vec<CommitParent>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct InnerCommit {
    #[serde(default)]
    pub message: String,
    pub tree: CommitTree,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CommitTree {
    pub sha: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CommitParent {
    pub sha: String,
}
