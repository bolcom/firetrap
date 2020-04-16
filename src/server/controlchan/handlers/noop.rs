//! The RFC 959 No Operation (`NOOP`) command
//
// This command does not affect any parameters or previously
// entered commands. It specifies no action other than that the
// server send an OK reply.

use super::handler::CommandContext;
use crate::server::controlchan::handlers::ControlCommandHandler;
use crate::server::controlchan::{Reply, ReplyCode};
use crate::server::error::FTPError;
use crate::storage;
use async_trait::async_trait;

pub struct Noop;

#[async_trait]
impl<S, U> ControlCommandHandler<S, U> for Noop
where
    U: Send + Sync + 'static,
    S: 'static + storage::StorageBackend<U> + Sync + Send,
    S::File: tokio::io::AsyncRead + Send,
    S::Metadata: storage::Metadata,
{
    async fn execute(&self, _args: CommandContext<S, U>) -> Result<Reply, FTPError> {
        Ok(Reply::new(ReplyCode::CommandOkay, "Successfully did nothing"))
    }
}
