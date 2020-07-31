use crate::{
    auth::UserDetail,
    server::{
        chancomms::InternalMsg,
        controlchan::{
            error::ControlChanError,
            handler::{CommandContext, CommandHandler},
            Reply, ReplyCode,
        },
    },
    storage::{Metadata, StorageBackend},
};
use async_trait::async_trait;
use chrono::{offset::Utc, DateTime};
use futures::{channel::mpsc::Sender, prelude::*};
use std::{path::PathBuf, sync::Arc};

const RFC3659_TIME: &str = "%Y%m%d%H%M%S";

#[derive(Debug)]
pub struct Mdtm {
    path: PathBuf,
}

impl Mdtm {
    pub fn new(path: PathBuf) -> Self {
        Mdtm { path }
    }
}

#[async_trait]
impl<S, U> CommandHandler<S, U> for Mdtm
where
    U: UserDetail,
    S: StorageBackend<U> + 'static,
    S::File: tokio::io::AsyncRead + Send + Sync,
    S::Metadata: 'static + Metadata,
{
    #[tracing_attributes::instrument]
    async fn handle(&self, args: CommandContext<S, U>) -> Result<Reply, ControlChanError> {
        let session = args.session.lock().await;
        let user = session.user.clone();
        let storage = Arc::clone(&session.storage);
        let path = session.cwd.join(self.path.clone());
        let mut tx_success: Sender<InternalMsg> = args.tx.clone();
        let mut tx_fail: Sender<InternalMsg> = args.tx.clone();
        let logger = args.logger;

        tokio::spawn(async move {
            match storage.metadata(&user, &path).await {
                Ok(metadata) => {
                    let modification_time = match metadata.modified() {
                        Ok(v) => Some(v),
                        Err(err) => {
                            if let Err(err) = tx_fail.send(InternalMsg::StorageError(err)).await {
                                slog::warn!(logger, "{}", err);
                            };
                            None
                        }
                    };

                    if let Some(mtime) = modification_time {
                        if let Err(err) = tx_success
                            .send(InternalMsg::CommandChannelReply(Reply::new_with_string(
                                ReplyCode::FileStatus,
                                DateTime::<Utc>::from(mtime).format(RFC3659_TIME).to_string(),
                            )))
                            .await
                        {
                            slog::warn!(logger, "{}", err);
                        }
                    }
                }
                Err(err) => {
                    if let Err(err) = tx_fail.send(InternalMsg::StorageError(err)).await {
                        slog::warn!(logger, "{}", err);
                    }
                }
            }
        });
        Ok(Reply::none())
    }
}
