use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConsoleError {
    #[error("weather source request failed: {0}")]
    Network(String),
    #[error("weather source returned invalid data: {0}")]
    InvalidData(String),
    #[error("local persistence failed: {0}")]
    Persistence(String),
    #[error("no cached weather data is available")]
    NoCache,
}

impl serde::Serialize for ConsoleError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
