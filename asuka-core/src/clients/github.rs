use crate::knowledge::Document;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::models::{self};
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

    pub async fn fetch_org_repos(&self, org: &str) -> Result<Vec<Document>> {
        let repos = self
            .client
            .orgs(org)
            .list_repos()
            .send()
            .await
            .context("Failed to fetch organization repositories")?
            .items;

        let mut documents = Vec::new();
        for repo in repos {
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
        }

        Ok(documents)
    }

    pub async fn fetch_repo_pulls(
        &self,
        owner: &str,
        repo: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<Document>> {
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
            .collect::<Vec<_>>();

        let mut documents = Vec::new();
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
                id: format!("github:pr:{}:{}/{}", owner, repo, pr.number),
                source_id: format!("github:{}/{}", owner, repo),
                content,
                created_at: pr.created_at,
                metadata: Some(json!(pr)),
            });
        }

        Ok(documents)
    }

    pub async fn fetch_repo_issues(
        &self,
        owner: &str,
        repo: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<Document>> {
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
            .filter(|issue| issue.updated_at >= since && issue.pull_request.is_none())
            .collect::<Vec<_>>();

        let mut documents = Vec::new();
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
                id: format!("github:issue:{}:{}/{}", owner, repo, issue.number),
                source_id: format!("github:{}/{}", owner, repo),
                content,
                created_at: Some(issue.created_at),
                metadata: Some(json!(issue)),
            });
        }

        Ok(documents)
    }

    pub async fn fetch_repo_commits(
        &self,
        owner: &str,
        repo: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<Document>> {
        let commits = self
            .client
            .repos(owner, repo)
            .list_commits()
            .since(since)
            .send()
            .await
            .context("Failed to fetch commits")?
            .items;

        let mut documents = Vec::new();
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
                id: format!("github:commit:{}:{}/{}", owner, repo, commit.sha),
                source_id: format!("github:{}/{}", owner, repo),
                content,
                created_at: author_date,
                metadata: Some(json!(commit)),
            });
        }

        Ok(documents)
    }

    pub async fn fetch_org_activity(
        &self,
        org: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<Document>> {
        let mut documents = Vec::new();
        let repos = self.fetch_org_repos(org).await?;
        documents.extend(repos.clone());

        for repo in repos {
            if let Some(metadata) = repo.metadata {
                if let Ok(repo_obj) = serde_json::from_value::<models::Repository>(metadata) {
                    let repo_name = repo_obj.full_name.as_deref().unwrap_or(&repo_obj.name);
                    let (owner, name) = repo_name.split_once('/').unwrap();

                    let pulls = self.fetch_repo_pulls(owner, name, since).await?;
                    documents.extend(pulls);

                    let issues = self.fetch_repo_issues(owner, name, since).await?;
                    documents.extend(issues);

                    let commits = self.fetch_repo_commits(owner, name, since).await?;
                    documents.extend(commits);
                }
            }
        }

        Ok(documents)
    }
}
