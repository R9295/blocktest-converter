#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("error parsing: {0}")]
    ParseError(String),
    #[error("reth provider failed: {err}")]
    ProviderError {
        #[source]
        err: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("conversion failure: {0}")]
    ConversionFailure(String),
}

impl Error {
    /// Create a new [`Error::Provider`] error.
    pub fn provider_error(
        err: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::ProviderError { err: Box::new(err) }
    }
}

