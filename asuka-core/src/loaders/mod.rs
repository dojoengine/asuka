pub mod github;
pub mod site;

use thiserror::Error;

use crate::knowledge::Document;
use rig::completion::CompletionModel;

#[derive(Error, Debug)]
pub enum LoaderError {
    #[error("{0}")]
    FileError(#[from] rig::loaders::file::FileLoaderError),

    #[cfg(feature = "pdf")]
    #[error("{0}")]
    PdfError(#[from] rig::loaders::pdf::PdfLoaderError),

    #[error("{0}")]
    GitError(#[from] github::GitLoaderError),

    #[error("{0}")]
    SiteError(#[from] site::SiteLoaderError),
}

pub struct MultiLoaderConfig {
    pub sources_path: String,
}

pub struct MultiLoader<M: CompletionModel> {
    config: MultiLoaderConfig,
    model: M,
}

impl<M: CompletionModel> MultiLoader<M> {
    pub fn new(config: MultiLoaderConfig, model: M) -> Self {
        Self { config, model }
    }

    pub async fn load_sources(
        &self,
        sources: Vec<String>,
    ) -> Result<impl Iterator<Item = Document>, LoaderError> {
        let mut documents = Vec::new();

        for source in sources {
            let parts: Vec<&str> = source.splitn(2, ':').collect();
            if parts.len() != 2 {
                continue;
            }

            let (source_type, url) = (parts[0], parts[1]);
            match source_type {
                "github" => {
                    let repo = github::GitLoader::new(url.to_string(), &self.config.sources_path)?;
                    documents.extend(
                        repo.with_root()?
                            .read_with_path()
                            .ignore_errors()
                            .into_iter()
                            .map(|(path, content)| Document {
                                id: path.to_string_lossy().to_string(),
                                source_id: format!("github:{}", url),
                                content,
                                created_at: None,
                            }),
                    );
                }
                "site" => {
                    let site = site::SiteLoader::new(url.to_string(), self.model.clone())?;
                    let content = site.extract_content().await?;
                    documents.push(Document {
                        id: url.to_string(),
                        source_id: format!("site:{}", url),
                        content,
                        created_at: None,
                    });
                }
                "file" => {
                    let loader = rig::loaders::file::FileLoader::with_glob(url)?;
                    documents.extend(loader.read_with_path().ignore_errors().into_iter().map(
                        |(path, content)| Document {
                            id: path.to_string_lossy().to_string(),
                            source_id: format!("file:{}", url),
                            content,
                            created_at: None,
                        },
                    ));
                }
                #[cfg(feature = "pdf")]
                "pdf" => {
                    let loader = rig::loaders::pdf::PdfFileLoader::with_glob(url)?;
                    documents.extend(loader.read_with_path().ignore_errors().into_iter().map(
                        |(path, content)| Document {
                            id: path.to_string_lossy().to_string(),
                            source_id: format!("pdf:{}", url),
                            content,
                            created_at: None,
                        },
                    ));
                }
                _ => continue,
            }
        }

        Ok(documents.into_iter())
    }
}
