//! Contains code pertaining to the FTP *data* channel

use super::chancomms::{DataCommand, InternalMsg};
use super::controlchan::command::Command;
use crate::auth::UserDetail;
use crate::server::Session;
use crate::storage::{self, Error, ErrorKind};

use futures::channel::mpsc::Sender;
use futures::prelude::*;
use log::info;
use log::{debug, warn};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub struct DataCommandExecutor<S, U>
where
    S: storage::StorageBackend<U>,
    S::File: tokio::io::AsyncRead + Send,
    S::Metadata: storage::Metadata,
    U: UserDetail,
{
    pub user: Arc<Option<U>>,
    pub socket: tokio::net::TcpStream,
    pub tls: bool,
    pub tx: Sender<InternalMsg>,
    pub storage: Arc<S>,
    pub cwd: PathBuf,
    pub start_pos: u64,
    pub identity_file: Option<PathBuf>,
    pub identity_password: Option<String>,
}

impl<S, U: Send + Sync + 'static> DataCommandExecutor<S, U>
where
    S: storage::StorageBackend<U> + Send + Sync + 'static,
    S::File: tokio::io::AsyncRead + Send,
    S::Metadata: storage::Metadata,
    U: UserDetail,
{
    pub async fn execute(self, cmd: Command) {
        match cmd {
            Command::Retr { path } => {
                self.exec_retr(path).await;
            }
            Command::Stor { path } => {
                self.exec_stor(path).await;
            }
            Command::List { path, .. } => {
                self.exec_list(path).await;
            }
            Command::Nlst { path } => {
                self.exec_nlst(path).await;
            }
            _ => unimplemented!(),
        }
    }

    async fn exec_retr(self, path: String) {
        let path = self.cwd.join(path);
        let mut tx_sending: Sender<InternalMsg> = self.tx.clone();
        let mut tx_error: Sender<InternalMsg> = self.tx.clone();
        tokio::spawn(async move {
            match self.storage.get(&self.user, path, self.start_pos).await {
                Ok(mut f) => match tx_sending.send(InternalMsg::SendingData).await {
                    Ok(_) => {
                        let mut output = Self::writer(self.socket, self.tls, self.identity_file, self.identity_password);
                        match tokio::io::copy(&mut f, &mut output).await {
                            Ok(bytes_copied) => {
                                if let Err(err) = output.shutdown().await {
                                    warn!("Could not shutdown output stream after RETR: {}", err);
                                }
                                if let Err(err) = tx_sending.send(InternalMsg::SendData { bytes: bytes_copied as i64 }).await {
                                    warn!("Could not notify control channel of successful RETR: {}", err);
                                }
                            }
                            Err(err) => warn!("Error copying streams during RETR: {}", err),
                        }
                    }
                    Err(err) => warn!("Error notifying control channel of progress during RETR: {}", err),
                },
                Err(err) => {
                    if let Err(err) = tx_error.send(InternalMsg::StorageError(err)).await {
                        warn!("Could not notify control channel of error with RETR: {}", err);
                    }
                }
            }
        });
    }

    async fn exec_stor(self, path: String) {
        let path = self.cwd.join(path);
        let mut tx_ok = self.tx.clone();
        let mut tx_error = self.tx.clone();
        tokio::spawn(async move {
            match self
                .storage
                .put(
                    &self.user,
                    Self::reader(self.socket, self.tls, self.identity_file, self.identity_password),
                    path,
                    self.start_pos,
                )
                .await
            {
                Ok(bytes) => {
                    if let Err(err) = tx_ok.send(InternalMsg::WrittenData { bytes: bytes as i64 }).await {
                        warn!("Could not notify control channel of successful STOR: {}", err);
                    }
                }
                Err(err) => {
                    if let Err(err) = tx_error.send(InternalMsg::StorageError(err)).await {
                        warn!("Could not notify control channel of error with STOR: {}", err);
                    }
                }
            }
        });
    }

    async fn exec_list(self, path: Option<String>) {
        let path = match path {
            Some(path) => self.cwd.join(path),
            None => self.cwd.clone(),
        };
        let mut tx_ok = self.tx.clone();
        tokio::spawn(async move {
            match self.storage.list_fmt(&self.user, path).await {
                Ok(cursor) => {
                    debug!("Copying future for List");
                    let mut input = cursor;
                    let mut output = Self::writer(self.socket, self.tls, self.identity_file, self.identity_password);
                    match tokio::io::copy(&mut input, &mut output).await {
                        Ok(_) => {
                            if let Err(err) = output.shutdown().await {
                                warn!("Could not shutdown output stream during LIST: {}", err);
                            }
                            if let Err(err) = tx_ok.send(InternalMsg::DirectorySuccessfullyListed).await {
                                warn!("Could not notify control channel of successful LIST: {}", err);
                            }
                        }
                        Err(err) => warn!("Could not copy from storage implementation during LIST: {}", err),
                    }
                }
                Err(err) => warn!("Failed to send directory list: {:?}", err),
            }
        });
    }

    async fn exec_nlst(self, path: Option<String>) {
        let path = match path {
            Some(path) => self.cwd.join(path),
            None => self.cwd.clone(),
        };
        let mut tx_ok = self.tx.clone();
        let mut tx_error = self.tx.clone();
        tokio::spawn(async move {
            match self.storage.nlst(&self.user, path).await {
                Ok(mut input) => {
                    let mut output = Self::writer(self.socket, self.tls, self.identity_file, self.identity_password);
                    match tokio::io::copy(&mut input, &mut output).await {
                        Ok(_) => {
                            if let Err(err) = output.shutdown().await {
                                warn!("Could not shutdown output stream during NLIST: {}", err);
                            }
                            if let Err(err) = tx_ok.send(InternalMsg::DirectorySuccessfullyListed).await {
                                warn!("Could not notify control channel of successful NLIST: {}", err);
                            }
                        }
                        Err(err) => warn!("Could not copy from storage implementation during NLST: {}", err),
                    }
                }
                Err(_) => {
                    if let Err(err) = tx_error.send(InternalMsg::StorageError(Error::from(ErrorKind::LocalError))).await {
                        warn!("Could not notify control channel of error with NLIST: {}", err);
                    }
                }
            }
        });
    }

    // Lots of code duplication here. Should disappear completely when the storage backends are rewritten in async/.await style
    fn writer(
        socket: tokio::net::TcpStream,
        tls: bool,
        identity_file: Option<PathBuf>,
        indentity_password: Option<String>,
    ) -> Box<dyn tokio::io::AsyncWrite + Send + Unpin + Sync> {
        if tls {
            let io = futures::executor::block_on(async move {
                let identity = crate::server::tls::identity(identity_file.unwrap(), indentity_password.unwrap());
                let acceptor = tokio_tls::TlsAcceptor::from(native_tls::TlsAcceptor::builder(identity).build().unwrap());
                acceptor.accept(socket).await.unwrap()
            });
            Box::new(io)
        } else {
            Box::new(socket)
        }
    }

    // Lots of code duplication here. Should disappear completely when the storage backends are rewritten in async/.await style
    fn reader(
        socket: tokio::net::TcpStream,
        tls: bool,
        identity_file: Option<PathBuf>,
        indentity_password: Option<String>,
    ) -> Box<dyn tokio::io::AsyncRead + Send + Unpin + Sync> {
        if tls {
            let io = futures::executor::block_on(async move {
                let identity = crate::server::tls::identity(identity_file.unwrap(), indentity_password.unwrap());
                let acceptor = tokio_tls::TlsAcceptor::from(native_tls::TlsAcceptor::builder(identity).build().unwrap());
                acceptor.accept(socket).await.unwrap()
            });
            Box::new(io)
        } else {
            Box::new(socket)
        }
    }
}

/// Processing for the data connection. This will spawn a new async task with the actual processing.
///
/// socket: the data socket we'll be working with
/// tls: tells if this should be a TLS connection
/// tx: channel to send the result of our operation to the control process
pub fn spawn_processing<S, U>(session: &mut Session<S, U>, socket: tokio::net::TcpStream, tx: Sender<InternalMsg>)
where
    S: storage::StorageBackend<U> + Send + Sync + 'static,
    S::File: tokio::io::AsyncRead + Send,
    S::Metadata: storage::Metadata,
    U: UserDetail + 'static,
{
    let mut data_cmd_rx = session.data_cmd_rx.take().unwrap().fuse();
    let mut data_abort_rx = session.data_abort_rx.take().unwrap().fuse();
    let tls = session.data_tls;
    let command_executor = DataCommandExecutor {
        user: session.user.clone(),
        socket,
        tls,
        tx,
        storage: Arc::clone(&session.storage),
        cwd: session.cwd.clone(),
        start_pos: session.start_pos,
        identity_file: if tls { Some(session.certs_file.clone().unwrap()) } else { None },
        identity_password: if tls { Some(session.certs_password.clone().unwrap()) } else { None },
    };

    tokio::spawn(async move {
        let mut timeout_delay = tokio::time::delay_for(std::time::Duration::from_secs(5 * 60));
        // TODO: Use configured timeout
        tokio::select! {
            Some(command) = data_cmd_rx.next() => {
                handle_incoming(DataCommand::ExternalCommand(command), command_executor).await;
            },
            Some(_) = data_abort_rx.next() => {
                handle_incoming(DataCommand::Abort, command_executor).await;
            },
            _ = &mut timeout_delay => {
                info!("Connection timed out");
                return;
            }
        };

        // This probably happened because the control channel was closed before we got here
        warn!("Nothing received");
    });
}

async fn handle_incoming<S, U>(incoming: DataCommand, command_executor: DataCommandExecutor<S, U>)
where
    S: storage::StorageBackend<U> + Send + Sync + 'static,
    S::File: tokio::io::AsyncRead + Send,
    S::Metadata: storage::Metadata,
    U: UserDetail + 'static,
{
    match incoming {
        DataCommand::Abort => {
            info!("Abort received");
        }
        DataCommand::ExternalCommand(command) => {
            info!("Data command received");
            command_executor.execute(command).await;
        }
    }
}
