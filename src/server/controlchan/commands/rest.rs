//! Restart of Interrupted Transfer (REST)
//! To avoid having to resend the entire file if the file is only
//! partially transferred, both sides need some way to agree on where in
//! the data stream to restart the data transfer.
//!
//! See also: https://cr.yp.to/ftp/retr.html
//!

use crate::{
    auth::UserDetail,
    server::controlchan::{
        error::ControlChanError,
        handler::{CommandContext, CommandHandler},
        Reply, ReplyCode,
    },
    storage::{Metadata, StorageBackend, FEATURE_RESTART},
};
use async_trait::async_trait;

#[derive(Debug)]
pub struct Rest {
    offset: u64,
}

impl Rest {
    pub fn new(offset: u64) -> Self {
        Rest { offset }
    }
}

#[async_trait]
impl<S, U> CommandHandler<S, U> for Rest
where
    U: UserDetail,
    S: StorageBackend<U> + 'static,
    S::Metadata: 'static + Metadata,
{
    #[tracing_attributes::instrument]
    async fn handle(&self, args: CommandContext<S, U>) -> Result<Reply, ControlChanError> {
        if args.storage_features & FEATURE_RESTART == 0 {
            return Ok(Reply::new(ReplyCode::CommandNotImplemented, "Not supported by the selected storage back-end."));
        }
        let mut session = args.session.lock().await;
        session.start_pos = self.offset;
        let msg = format!("Restarting at {}. Now send STORE or RETRIEVE.", self.offset);
        Ok(Reply::new(ReplyCode::FileActionPending, &*msg))
    }
}
