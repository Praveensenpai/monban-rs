use thiserror::Error;

#[derive(Error, Debug)]
pub enum MonbanError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("ONNX Runtime error: {0}")]
    Ort(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Stream error: {0}")]
    Stream(String),
}

impl<T> From<ort::Error<T>> for MonbanError {
    fn from(err: ort::Error<T>) -> Self {
        MonbanError::Ort(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MonbanError>;
