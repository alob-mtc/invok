use crate::shared::error::{AppResult, RuntimeError};
use bollard::{container::LogsOptions, Docker};
use futures_util::stream::{Stream, StreamExt};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{error, info, warn};

/// Log stream message containing either log content or an error
#[derive(Debug, Clone)]
pub enum LogMessage {
    /// Log content from the container
    Content(String),
    /// Error occurred while streaming logs
    Error(String),
    /// Stream has ended
    End,
}

/// Container log streamer that handles Docker container log streaming
pub struct ContainerLogStreamer {
    docker: Docker,
}

impl ContainerLogStreamer {
    /// Create a new container log streamer
    pub fn new() -> AppResult<Self> {
        let docker = Docker::connect_with_http_defaults()
            .map_err(|e| RuntimeError::System(format!("Failed to connect to Docker: {}", e)))?;

        Ok(Self { docker })
    }

    /// Create a new container log streamer with existing Docker client
    pub fn with_docker(docker: Docker) -> Self {
        Self { docker }
    }

    /// Stream logs from a container
    ///
    /// # Arguments
    ///
    /// * `container_id` - The ID of the container to stream logs from
    /// * `follow` - Whether to follow the log stream (true for real-time streaming)
    ///
    /// # Returns
    ///
    /// A stream of LogMessage items
    pub async fn stream_logs(
        &self,
        container_id: &str,
        follow: bool,
    ) -> AppResult<impl Stream<Item = LogMessage>> {
        info!(
            container_id = %container_id,
            follow = follow,
            "Starting container log stream"
        );

        let options = Some(LogsOptions::<String> {
            stdout: true,
            stderr: true,
            follow,
            timestamps: false,
            ..Default::default()
        });

        let logs_stream = self.docker.logs(container_id, options);
        let (tx, rx) = mpsc::unbounded_channel();
        let container_id = container_id.to_string();

        // Spawn task to handle Docker log stream
        tokio::spawn(async move {
            let mut stream = logs_stream;

            // Send initial connection message
            let _ = tx.send(LogMessage::Content(
                "Connected to container logs".to_string(),
            ));

            while let Some(log_result) = stream.next().await {
                match log_result {
                    Ok(log_output) => {
                        let text = log_output.to_string();

                        // Clean up the log text (remove extra whitespace)
                        let clean_text = text.trim();
                        if !clean_text.is_empty() {
                            if tx
                                .send(LogMessage::Content(clean_text.to_string()))
                                .is_err()
                            {
                                // Client disconnected
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            container_id = %container_id,
                            error = %e,
                            "Error reading container logs"
                        );
                        let _ = tx.send(LogMessage::Error(format!("Log stream error: {}", e)));
                        break;
                    }
                }
            }

            // Send stream end message
            let _ = tx.send(LogMessage::End);
            info!(
                container_id = %container_id,
                "Container log stream ended"
            );
        });

        Ok(UnboundedReceiverStream::new(rx))
    }
}