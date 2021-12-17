//! The event system broadcasts change notifications to everyone listening.

use anyhow::Context as _;
use async_stream::stream;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgListener, PgExecutor, PgPool};
use std::{sync::Arc, time::Duration};
use tokio::time;

#[derive(Serialize)]
pub(crate) enum Notify<'a> {
    UpdateUser { id: i64, email: &'a str },
    DeleteUser { id: i64 },
    ChangedCards { for_user: i64 },
}

impl Notify<'_> {
    pub(crate) async fn broadcast(self, database: impl PgExecutor<'_>) -> anyhow::Result<()> {
        sqlx::query("SELECT pg_notify('events', $1)")
            .bind(serde_json::to_string(&self).expect("serializing event as JSON failed"))
            .execute(database)
            .await
            .context("failed to broadcast event")?;
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
pub(crate) enum Received {
    UpdateUser {
        id: i64,
        email: Arc<str>,
    },
    DeleteUser {
        id: i64,
    },
    ChangedCards {
        for_user: i64,
    },
    #[serde(skip_deserializing)]
    Lapsed,
}

pub(crate) async fn subscribe(database: PgPool) -> anyhow::Result<impl Stream<Item = Received>> {
    let mut listener = PgListener::connect_with(&database)
        .await
        .context("couldn't subscribe to database")?;

    listener
        .listen("events")
        .await
        .context("couldn't listen to notifications channel")?;

    Ok(stream! {
        loop {
            let notification = match listener.try_recv().await {
                Ok(notification) => notification,
                Err(e) => {
                    log::error!(
                        "{:?}",
                        anyhow::Error::new(e).context("couldn't receive notification from database")
                    );
                    time::sleep(Duration::from_secs(10)).await;
                    None
                }
            };

            let notification = match notification {
                Some(notification) => notification,
                None => {
                    log::warn!("Connection to database interrupted");
                    yield Received::Lapsed;
                    continue;
                }
            };

            yield match serde_json::from_str(notification.payload()) {
                Ok(event) => event,
                Err(e) => {
                    log::error!("failed to deserialize notification event: {}", e);
                    continue;
                }
            };
        }
    })
}
