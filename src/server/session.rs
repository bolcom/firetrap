//! The session module implements per-connection session handling and currently also
//! implements the control loop for the *data* channel.

use super::chancomms::{DataCommand, InternalMsg};
use super::commands::Command;
use super::storage::AsAsyncReads;
use super::stream::SwitchingTlsStream;
use crate::metrics;
use crate::storage::{self, Error, ErrorKind};
use futures::prelude::*;
use futures::sync::mpsc::Sender;
use futures03::channel::mpsc::Receiver;
use log::{debug, warn};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpStream;

const DATA_CHANNEL_ID: u8 = 1;

#[derive(PartialEq)]
pub enum SessionState {
    New,
    WaitPass,
    WaitCmd,
}

// This is where we keep the state for a ftp session.
pub struct Session<S, U: Send + Sync>
where
    S: storage::StorageBackend<U>,
    S::File: crate::storage::AsAsyncReads + Send,
    S::Metadata: storage::Metadata,
{
    pub user: Arc<Option<U>>,
    pub username: Option<String>,
    pub storage: Arc<S>,
    pub data_cmd_tx: Option<futures03::channel::mpsc::Sender<Command>>,
    pub data_cmd_rx: Option<Receiver<Command>>,
    pub data_abort_tx: Option<futures03::channel::mpsc::Sender<()>>,
    pub data_abort_rx: Option<Receiver<()>>,
    pub cwd: std::path::PathBuf,
    pub rename_from: Option<PathBuf>,
    pub state: SessionState,
    pub certs_file: Option<PathBuf>,
    pub key_file: Option<PathBuf>,
    // True if the command channel is in secure mode
    pub cmd_tls: bool,
    // True if the data channel is in secure mode.
    pub data_tls: bool,
    pub with_metrics: bool,
    // The starting byte for a STOR or RETR command. Set by the _Restart of Interrupted Transfer (REST)_
    // command to support resume functionality.
    pub start_pos: u64,
}

impl<S, U: Send + Sync + 'static> Session<S, U>
where
    S: storage::StorageBackend<U> + Send + Sync + 'static,
    S::File: crate::storage::AsAsyncReads + Send,
    S::Metadata: storage::Metadata,
{
    pub(super) fn with_storage(storage: Arc<S>) -> Self {
        Session {
            user: Arc::new(None),
            username: None,
            storage,
            data_cmd_tx: None,
            data_cmd_rx: None,
            data_abort_tx: None,
            data_abort_rx: None,
            cwd: "/".into(),
            rename_from: None,
            state: SessionState::New,
            certs_file: Option::None,
            key_file: Option::None,
            cmd_tls: false,
            data_tls: false,
            with_metrics: false,
            start_pos: 0,
        }
    }

    pub(super) fn certs(mut self, certs_file: Option<PathBuf>, key_file: Option<PathBuf>) -> Self {
        self.certs_file = certs_file;
        self.key_file = key_file;
        self
    }

    pub(super) fn with_metrics(mut self, with_metrics: bool) -> Self {
        if with_metrics {
            metrics::inc_session();
        }
        self.with_metrics = with_metrics;
        self
    }

    /// Processing for the data connection.
    ///
    /// socket: the data socket we'll be working with
    /// tls: tells if this should be a TLS connection
    /// tx: channel to send the result of our operation to the control process
    pub(super) fn process_data(&mut self, user: Arc<Option<U>>, socket: TcpStream, tls: bool, tx: Sender<InternalMsg>) {
        let tcp_tls_stream: Box<dyn crate::server::io::AsyncStream> = match (tls, &self.certs_file, &self.key_file) {
            (true, Some(certs), Some(keys)) => Box::new(SwitchingTlsStream::new(socket, DATA_CHANNEL_ID, certs, keys)),
            _ => Box::new(socket),
        };

        // TODO: Either take the rx as argument, or properly check the result instead of
        // `unwrap()`.
        let rx = {
            use futures03::stream::StreamExt;
            use futures03::stream::TryStreamExt;
            self.data_cmd_rx.take().unwrap().map(Ok::<Command, ()>).compat()
        };
        // TODO: Same as above, don't `unwrap()` here. Ideally we solve this by refactoring to a
        // proper state machine.
        let abort_rx: Receiver<()> = self.data_abort_rx.take().unwrap();
        let storage: Arc<S> = Arc::clone(&self.storage);
        let cwd = self.cwd.clone();
        let start_pos: u64 = self.start_pos;
        let task = rx
            .take(1)
            .map(DataCommand::ExternalCommand)
            .select({
                use futures03::stream::StreamExt;
                use futures03::stream::TryStreamExt;
                abort_rx.map(|_| Ok(DataCommand::Abort)).compat()
            })
            .take_while(|data_cmd| Ok(*data_cmd != DataCommand::Abort))
            .into_future()
            .map(move |(cmd, _)| {
                use self::DataCommand::ExternalCommand;
                match cmd {
                    Some(ExternalCommand(Command::Retr { path })) => {
                        let path = cwd.join(path);
                        let tx_sending: Sender<InternalMsg> = tx.clone();
                        let tx_error: Sender<InternalMsg> = tx.clone();
                        tokio::spawn(
                            storage
                                .get(&user, path, start_pos)
                                .and_then(|f| {
                                    tx_sending
                                        .send(InternalMsg::SendingData)
                                        .map_err(|_e| Error::from(ErrorKind::LocalError))
                                        .and_then(|_| {
                                            tokio::io::copy(f.as_tokio01_async_read(), tcp_tls_stream).map_err(|_| Error::from(ErrorKind::LocalError))
                                        })
                                        .and_then(|(bytes, _, _)| {
                                            tx.send(InternalMsg::SendData { bytes: bytes as i64 })
                                                .map_err(|_| Error::from(ErrorKind::LocalError))
                                        })
                                })
                                .or_else(|e| tx_error.send(InternalMsg::StorageError(e)))
                                .map(|_| ())
                                .map_err(|e| {
                                    warn!("Failed to send file: {:?}", e);
                                }),
                        );
                    }
                    Some(ExternalCommand(Command::Stor { path })) => {
                        let path = cwd.join(path);
                        let tx_ok = tx.clone();
                        let tx_error = tx.clone();
                        tokio::spawn(
                            storage
                                .put(&user, tcp_tls_stream, path, start_pos)
                                .and_then(|bytes| {
                                    tx_ok
                                        .send(InternalMsg::WrittenData { bytes: bytes as i64 })
                                        .map_err(|_| Error::from(ErrorKind::LocalError))
                                })
                                .or_else(|e| tx_error.send(InternalMsg::StorageError(e)))
                                .map(|_| ())
                                .map_err(|e| {
                                    warn!("Failed to send file: {:?}", e);
                                }),
                        );
                    }
                    Some(ExternalCommand(Command::List { path, .. })) => {
                        let path = match path {
                            Some(path) => cwd.join(path),
                            None => cwd,
                        };
                        let tx_ok = tx.clone();
                        let tx_error = tx.clone();
                        tokio::spawn(
                            storage
                                .list_fmt(&user, path)
                                .and_then(|cursor| {
                                    debug!("Copying future for List");
                                    tokio::io::copy(cursor, tcp_tls_stream)
                                })
                                .and_then(|reader_writer| {
                                    debug!("Shutdown future for List");
                                    let tcp_tls_stream = reader_writer.2;
                                    tokio::io::shutdown(tcp_tls_stream)
                                })
                                .map_err(|_| Error::from(ErrorKind::LocalError))
                                .and_then(|_| {
                                    tx_ok
                                        .send(InternalMsg::DirectorySuccessfullyListed)
                                        .map_err(|_| Error::from(ErrorKind::LocalError))
                                })
                                .or_else(|e| tx_error.send(InternalMsg::StorageError(e)))
                                .map(|_| ())
                                .map_err(|e| {
                                    warn!("Failed to send directory list: {:?}", e);
                                }),
                        );
                    }
                    Some(ExternalCommand(Command::Nlst { path })) => {
                        let path = match path {
                            Some(path) => cwd.join(path),
                            None => cwd,
                        };
                        let tx_ok = tx.clone();
                        let tx_error = tx.clone();
                        tokio::spawn(
                            storage
                                .nlst(&user, path)
                                .and_then(|res| tokio::io::copy(res, tcp_tls_stream))
                                .map_err(|_| Error::from(ErrorKind::LocalError))
                                .and_then(|_| {
                                    tx_ok
                                        .send(InternalMsg::DirectorySuccessfullyListed)
                                        .map_err(|_| Error::from(ErrorKind::LocalError))
                                })
                                .or_else(|e| tx_error.send(InternalMsg::StorageError(e)))
                                .map(|_| ())
                                .map_err(|e| {
                                    warn!("Failed to send directory list: {:?}", e);
                                }),
                        );
                    }
                    // TODO: Remove catch-all Some(_) when I'm done implementing :)
                    Some(ExternalCommand(_)) => unimplemented!(),
                    Some(DataCommand::Abort) => unreachable!(),
                    None => { /* This probably happened because the control channel was closed before we got here */ }
                }
            })
            .into_future()
            .map_err(|_| ())
            .map(|_| ());

        tokio::spawn(task);
    }
}

impl<S, U: Send + Sync> Drop for Session<S, U>
where
    S: storage::StorageBackend<U>,
    S::File: crate::storage::AsAsyncReads + Send,
    S::Metadata: storage::Metadata,
{
    fn drop(&mut self) {
        if self.with_metrics {
            // Decrease the sessions metrics gauge when the session goes out of scope.
            metrics::dec_session();
        }
    }
}
