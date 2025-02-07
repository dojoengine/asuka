use crate::knowledge::Document;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::models::{self, issues, pulls, repos};
use octocrab::Octocrab;
use serde_json::json;

#[derive(Clone)]
pub struct GitHubClient {
    client: Octocrab,
}

impl GitHubClient {
    pub fn new(token: String) -> Self {
        Self {
            client: Octocrab::builder()
                .personal_token(token)
                .build()
                .expect("Failed to create GitHub client"),
        }
    }

    pub async fn fetch_org_repos(&self, org: &str) -> Result<Vec<models::Repository>> {
        let repos = self
            .client
            .orgs(org)
            .list_repos()
            .send()
            .await
            .context("Failed to fetch organization repositories")?
            .items;

        Ok(repos)
    }

    pub async fn fetch_repo_pulls(
        &self,
        owner: &str,
        repo: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<pulls::PullRequest>> {
        let pulls = self
            .client
            .pulls(owner, repo)
            .list()
            .state(octocrab::params::State::All)
            .sort(octocrab::params::pulls::Sort::Updated)
            .direction(octocrab::params::Direction::Descending)
            .send()
            .await
            .context("Failed to fetch pull requests")?
            .items
            .into_iter()
            .filter(|pr| pr.updated_at.map(|d| d >= since).unwrap_or(false))
            .collect();

        Ok(pulls)
    }

    pub async fn fetch_repo_issues(
        &self,
        owner: &str,
        repo: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<issues::Issue>> {
        let issues = self
            .client
            .issues(owner, repo)
            .list()
            .state(octocrab::params::State::All)
            .sort(octocrab::params::issues::Sort::Updated)
            .direction(octocrab::params::Direction::Descending)
            .send()
            .await
            .context("Failed to fetch issues")?
            .items
            .into_iter()
            .filter(|issue| issue.updated_at >= since && issue.pull_request.is_none()) // Exclude PRs
            .collect();

        Ok(issues)
    }

    pub async fn fetch_repo_commits(
        &self,
        owner: &str,
        repo: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<repos::RepoCommit>> {
        let commits = self
            .client
            .repos(owner, repo)
            .list_commits()
            .since(since)
            .send()
            .await
            .context("Failed to fetch commits")?
            .items;

        Ok(commits)
    }

    pub async fn fetch_org_activity(
        &self,
        org: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<Document>> {
        let mut documents = Vec::new();
        let repos = self.fetch_org_repos(org).await?;

        // Create repository documents
        for repo in &repos {
            let repo_name = repo.full_name.as_deref().unwrap_or(&repo.name);
            let html_url = repo
                .html_url
                .as_ref()
                .map(|url| url.to_string())
                .unwrap_or_default();

            let content = format!(
                "Repository: {}\nDescription: {}\nURL: {}\nCreated: {}\nLast Updated: {}",
                repo_name,
                repo.description.as_deref().unwrap_or("No description"),
                html_url,
                repo.created_at.unwrap_or_default(),
                repo.updated_at.unwrap_or_default()
            );

            documents.push(Document {
                id: format!("github:repo:{}", repo_name),
                source_id: format!("github:{}", org),
                content,
                created_at: repo.created_at,
                metadata: Some(json!(repo)),
            });

            let (owner, name) = repo_name.split_once('/').unwrap();

            // Fetch and create PR documents
            let pulls = self.fetch_repo_pulls(owner, name, since).await?;
            for pr in pulls {
                let content = format!(
                    "Pull Request: #{} - {}\nAuthor: @{}\nState: {}\nURL: {}\nCreated: {}\nLast Updated: {}\n\n{}",
                    pr.number,
                    pr.title.as_deref().unwrap_or_default(),
                    pr.user.as_ref().map(|u| u.login.clone()).unwrap_or_default(),
                    pr.state.as_ref().map(|s| format!("{:?}", s)).unwrap_or_else(|| "unknown".to_string()),
                    pr.html_url.as_ref().map(|url| url.to_string()).unwrap_or_default(),
                    pr.created_at.unwrap_or_default(),
                    pr.updated_at.unwrap_or_default(),
                    pr.body.as_deref().unwrap_or_default()
                );

                documents.push(Document {
                    id: format!("github:pr:{}:{}", repo_name, pr.number),
                    source_id: format!("github:{}", org),
                    content,
                    created_at: pr.created_at,
                    metadata: Some(json!(pr)),
                });
            }

            // Fetch and create issue documents
            let issues = self.fetch_repo_issues(owner, name, since).await?;
            for issue in issues {
                let content = format!(
                    "Issue: #{} - {}\nAuthor: @{}\nState: {}\nURL: {}\nCreated: {}\nLast Updated: {}\n\n{}",
                    issue.number,
                    issue.title,
                    issue.user.login,
                    format!("{:?}", issue.state),
                    issue.html_url,
                    issue.created_at,
                    issue.updated_at,
                    issue.body.as_deref().unwrap_or_default()
                );

                documents.push(Document {
                    id: format!("github:issue:{}:{}", repo_name, issue.number),
                    source_id: format!("github:{}", org),
                    content,
                    created_at: Some(issue.created_at),
                    metadata: Some(json!(issue)),
                });
            }

            // Fetch and create commit documents
            let commits = self.fetch_repo_commits(owner, name, since).await?;
            for commit in commits {
                let author_date = commit.commit.author.as_ref().and_then(|a| a.date);

                let author_name = commit
                    .author
                    .as_ref()
                    .map(|a| format!("@{}", a.login))
                    .unwrap_or_else(|| {
                        commit
                            .commit
                            .author
                            .as_ref()
                            .map(|a| a.name.clone())
                            .unwrap_or_default()
                    });

                let content = format!(
                    "Commit: {}\nAuthor: {}\nDate: {}\nURL: {}\n\n{}",
                    commit.sha,
                    author_name,
                    author_date.unwrap_or_default(),
                    commit.html_url,
                    commit.commit.message
                );

                documents.push(Document {
                    id: format!("github:commit:{}:{}", repo_name, commit.sha),
                    source_id: format!("github:{}", org),
                    content,
                    created_at: author_date,
                    metadata: Some(json!(commit)),
                });
            }
        }

        Ok(documents)
    }
}
