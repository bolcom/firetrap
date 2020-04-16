//! The RFC 959 Retrieve (`RETR`) command
//
// This command causes the server-DTP to transfer a copy of the
// file, specified in the pathname, to the server- or user-DTP
// at the other end of the data connection.  The status and
// contents of the file at the server site shall be unaffected.

use super::handler::CommandContext;
use crate::server::controlchan::command::Command;
use crate::server::controlchan::handlers::ControlCommandHandler;
use crate::server::controlchan::Reply;
use crate::server::error::{FTPError, FTPErrorKind};
use crate::storage;
use async_trait::async_trait;
use futures::prelude::*;
use log::warn;

pub struct Retr;

#[async_trait]
impl<S, U> ControlCommandHandler<S, U> for Retr
where
    U: Send + Sync + 'static,
    S: 'static + storage::StorageBackend<U> + Sync + Send,
    S::File: tokio::io::AsyncRead + Send,
    S::Metadata: storage::Metadata,
{
    async fn execute(&self, args: CommandContext<S, U>) -> Result<Reply, FTPError> {
        let mut session = args.session.lock().await;
        let cmd: Command = args.cmd.clone();
        match session.data_cmd_tx.take() {
            Some(mut tx) => {
                tokio::spawn(async move {
                    if let Err(err) = tx.send(cmd).await {
                        warn!("{}", err);
                    }
                });
                Ok(Reply::none())
            }
            None => Err(FTPErrorKind::InternalServerError.into()),
        }
    }
}
