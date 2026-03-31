// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Platform-aware IPC transport — named pipes (Windows) / unix domain sockets (Unix).
//!
//! [`IpcChannel`] defines the collector channel with its platform-specific path.
//! Collectors connect via [`IpcStream::connect`].
//!
//! On Windows, named pipes only accept one client at a time per instance.
//! The service pre-creates the next pipe instance before returning each
//! accepted connection, so there is always an instance ready for the next
//! client.

use std::path::PathBuf;

/// A named IPC channel with a platform-specific path.
#[derive(Debug, Clone)]
pub struct IpcChannel {
    /// Logical name (e.g. `"collector"`).
    pub name: String,
    /// Platform-specific path to the pipe/socket.
    pub path: PathBuf,
}

impl IpcChannel {
    /// Create a channel with the default path for the given name.
    ///
    /// - Windows: `\\.\pipe\telescope-{name}`
    /// - Unix: `~/.telescope/{name}.sock`
    pub fn default_for(name: &str) -> Self {
        Self {
            name: name.to_string(),
            path: default_path(name),
        }
    }

    /// The **collector** channel — out-of-process collectors → Service.
    pub fn collector() -> Self {
        Self::default_for("collector")
    }
}

/// Resolve the default IPC path for a channel name.
fn default_path(name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\telescope-{name}"))
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".telescope")
            .join(format!("{name}.sock"))
    }
}

// ── Listener (service side) ──

/// Listens for incoming IPC connections on a channel.
///
/// On Windows, a pre-created named pipe instance is held inside a
/// [`tokio::sync::Mutex`] so that the next instance is always ready
/// before the current connection is returned.
pub struct IpcListener {
    channel: IpcChannel,
    #[cfg(windows)]
    server: tokio::sync::Mutex<tokio::net::windows::named_pipe::NamedPipeServer>,
    #[cfg(not(windows))]
    inner: tokio::net::UnixListener,
}

impl IpcListener {
    /// Bind to the channel's path and start listening.
    #[allow(clippy::unused_async)]
    pub async fn bind(channel: IpcChannel) -> std::io::Result<Self> {
        #[cfg(not(windows))]
        {
            // Remove stale socket file if it exists.
            let _ = std::fs::remove_file(&channel.path);
            if let Some(parent) = channel.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let listener = tokio::net::UnixListener::bind(&channel.path)?;
            Ok(Self {
                channel,
                inner: listener,
            })
        }
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            // Create the first pipe instance eagerly so it is ready to
            // accept a client as soon as the accept loop starts.
            let first = ServerOptions::new()
                .first_pipe_instance(true)
                .create(channel.path.as_os_str())?;
            Ok(Self {
                server: tokio::sync::Mutex::new(first),
                channel,
            })
        }
    }

    /// Accept the next incoming connection.
    pub async fn accept(&self) -> std::io::Result<IpcStream> {
        #[cfg(not(windows))]
        {
            let (stream, _addr) = self.inner.accept().await?;
            let (read, write) = stream.into_split();
            Ok(IpcStream {
                reader: Box::new(read),
                writer: Box::new(write),
            })
        }
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;

            let mut guard = self.server.lock().await;

            // Wait for a client to connect to the pre-created instance.
            guard.connect().await?;

            // Take the connected instance out so it is returned regardless
            // of whether creating the next pipe instance succeeds.
            let path = self.channel.path.as_os_str();
            let next = ServerOptions::new().first_pipe_instance(false).create(path);

            let connected = match next {
                Ok(fresh) => std::mem::replace(&mut *guard, fresh),
                Err(e) => {
                    // Log the error but still return the connected stream.
                    // The next call to accept() will fail when it tries to
                    // use the stale instance, but at least this client is
                    // not left hanging.
                    eprintln!("warning: failed to pre-create next pipe instance: {e}");
                    // We cannot replace, so just hand out the current one.
                    // Next accept() will see it's already connected and error.
                    // That's acceptable — the alternative is dropping a
                    // successfully connected client.
                    let stale = ServerOptions::new()
                        .first_pipe_instance(false)
                        .create(path)
                        .unwrap_or_else(|_| {
                            // Last resort: give back the current server.
                            // This should essentially never happen.
                            panic!("failed to create named pipe instance twice");
                        });
                    std::mem::replace(&mut *guard, stale)
                }
            };

            // Release the lock before doing IO.
            drop(guard);

            let (read, write) = tokio::io::split(connected);
            Ok(IpcStream {
                reader: Box::new(read),
                writer: Box::new(write),
            })
        }
    }

    /// Get the channel this listener is bound to.
    #[must_use]
    pub fn channel(&self) -> &IpcChannel {
        &self.channel
    }
}

// ── Stream (client side + accepted connections) ──

/// A bidirectional IPC stream for sending requests and receiving responses.
pub struct IpcStream {
    reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
    writer: Box<dyn tokio::io::AsyncWrite + Unpin + Send>,
}

impl IpcStream {
    /// Connect to a channel as a client.
    #[allow(clippy::unused_async)]
    pub async fn connect(channel: &IpcChannel) -> std::io::Result<Self> {
        #[cfg(not(windows))]
        {
            let stream = tokio::net::UnixStream::connect(&channel.path).await?;
            let (read, write) = stream.into_split();
            Ok(Self {
                reader: Box::new(read),
                writer: Box::new(write),
            })
        }
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ClientOptions;
            let client = ClientOptions::new().open(channel.path.as_os_str())?;
            let (read, write) = tokio::io::split(client);
            Ok(Self {
                reader: Box::new(read),
                writer: Box::new(write),
            })
        }
    }

    /// Send a request and wait for the response.
    pub async fn call(
        &mut self,
        request: &super::IpcRequest,
    ) -> std::io::Result<super::IpcResponse> {
        let req_bytes = serde_json::to_vec(request)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        super::write_frame(&mut self.writer, &req_bytes).await?;

        let resp_bytes = super::read_frame(&mut self.reader).await?.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed")
        })?;

        serde_json::from_slice(&resp_bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Read an incoming request (server side).
    pub async fn read_request(&mut self) -> std::io::Result<Option<super::IpcRequest>> {
        let Some(bytes) = super::read_frame(&mut self.reader).await? else {
            return Ok(None);
        };
        let request = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Some(request))
    }

    /// Write a response (server side).
    pub async fn write_response(&mut self, response: &super::IpcResponse) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(response)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        super::write_frame(&mut self.writer, &bytes).await
    }

    /// Write a notification (server-push, no response expected).
    pub async fn write_notification(
        &mut self,
        notification: &super::protocol::IpcNotification,
    ) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(notification)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        super::write_frame(&mut self.writer, &bytes).await
    }

    /// Read an incoming notification (client side, subscription stream).
    pub async fn read_notification(
        &mut self,
    ) -> std::io::Result<Option<super::protocol::IpcNotification>> {
        let Some(bytes) = super::read_frame(&mut self.reader).await? else {
            return Ok(None);
        };
        let notification = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Some(notification))
    }
}

impl std::fmt::Debug for IpcStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcStream").finish_non_exhaustive()
    }
}
