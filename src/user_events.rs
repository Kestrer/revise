//! Websocket endpoint that gives a stream of user events.

use crate::{session::Session, utils::EndpointResult};
use anyhow::{anyhow, Context as _};
use async_stream::stream;
use axum::{
    extract::{
        ws::{self, WebSocket},
        WebSocketUpgrade,
    },
    routing, Router,
};
use futures_util::{stream, StreamExt as _, TryStreamExt as _};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgListener, PgExecutor, PgPool};
use std::{
    collections::hash_map::{self, HashMap},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::{pin, sync::broadcast, task, time};

pub(crate) async fn routes(database: PgPool) -> anyhow::Result<Router> {
    let mut listener = PgListener::connect_with(&database)
        .await
        .context("couldn't subscribe to database")?;
    listener
        .listen("user_events")
        .await
        .context("couldn't listen to notifications channel")?;

    let notifications = Arc::new(Mutex::new(HashMap::new()));

    let task = Arc::new(AbortOnDrop(task::spawn(run_manager(
        listener,
        notifications.clone(),
    ))));

    Ok(Router::new().route(
        "/",
        routing::get(|session: Session, ws: WebSocketUpgrade| async move {
            let _task = task;
            let user_id = session.user_id(&database).await?;
            let receiver = match notifications.lock().entry(user_id) {
                hash_map::Entry::Vacant(entry) => {
                    let (sender, receiver) = broadcast::channel(16);
                    entry.insert(sender);
                    receiver
                }
                hash_map::Entry::Occupied(entry) => entry.get().subscribe(),
            };

            EndpointResult::Ok(ws.on_upgrade(move |ws| async move {
                if let Err(e) = handle_ws(database, user_id, receiver, ws).await {
                    log::error!("ws stream ended with error: {:?}", anyhow!(e));
                }
            }))
        }),
    ))
}

#[derive(Clone)]
enum UserEvent {
    UpdateUser { email: Arc<str> },
    DeleteUser,
    ChangedCards,
    Lapsed,
}

async fn run_manager(
    mut listener: PgListener,
    notifications: Arc<Mutex<HashMap<i64, broadcast::Sender<UserEvent>>>>,
) {
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
                for sender in notifications.lock().values() {
                    let _ = sender.send(UserEvent::Lapsed);
                }
                continue;
            }
        };

        let mut parts = notification.payload().splitn(3, ' ');

        let user = parts
            .next()
            .unwrap()
            .parse()
            .expect("invalid notification user id");
        let event = match parts.next().expect("no notification name") {
            "UpdateUser" => {
                let email = parts.next().expect("UpdateUser without email");
                UserEvent::UpdateUser {
                    email: Arc::from(email),
                }
            }
            "DeleteUser" => UserEvent::DeleteUser,
            "ChangedCards" => UserEvent::ChangedCards,
            kind => {
                log::error!("unknown user event kind {}", kind);
                continue;
            }
        };
        assert_eq!(parts.next(), None);

        if let Some(sender) = notifications.lock().get(&user) {
            let _ = sender.send(event);
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum WsRequest {
    SetQueryOpts {
        #[serde(flatten)]
        opts: QueryOpts,
    },
}

#[derive(Clone, Copy, Deserialize)]
struct QueryOpts {
    // TODO: disallow negatives
    limit: i64,
    offset: i64,
}

#[derive(Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum WsResponse {
    Update {
        #[serde(skip_serializing_if = "Option::is_none")]
        email: Option<Arc<str>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cards: Option<Vec<Card>>,
    },
    DeleteUser,
    Error {
        message: String,
    },
}

#[derive(sqlx::FromRow, Serialize)]
struct Card {
    id: i64,
    created_at: i64,
    terms: String,
    definitions: String,
    case_sensitive: bool,
    knowledge: i16,
    safety_net: bool,
}

async fn handle_ws(
    database: PgPool,
    user_id: i64,
    mut events: broadcast::Receiver<UserEvent>,
    ws: WebSocket,
) -> anyhow::Result<()> {
    enum Input {
        UserEvent(UserEvent),
        WebSocket(Vec<u8>),
        Exit,
    }

    let mut query_opts = None;

    let inputs = stream::select(
        stream! {
            loop {
                match events.recv().await {
                    Ok(event) => yield Ok(Input::UserEvent(event)),
                    Err(broadcast::error::RecvError::Closed) => {
                        yield Ok(Input::Exit);
                        break;
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        yield Ok(Input::UserEvent(UserEvent::Lapsed));
                    },
                }
            }
        },
        ws.try_filter_map(|msg| async move {
            Ok(match msg {
                ws::Message::Text(s) => Some(Input::WebSocket(s.into_bytes())),
                ws::Message::Binary(v) => Some(Input::WebSocket(v)),
                ws::Message::Ping(_) | ws::Message::Pong(_) => None,
                ws::Message::Close(_) => Some(Input::Exit),
            })
        }),
    );

    pin!(inputs);

    async fn send_ws<A, B, C>(
        mut stream: Pin<&mut stream::Select<A, stream::TryFilterMap<WebSocket, B, C>>>,
        msg: WsResponse,
    ) -> anyhow::Result<()> {
        let json = serde_json::to_string(&msg).expect("serializing ws response as json failed");
        let msg = ws::Message::Text(json);
        let mut stream = stream.as_mut().get_pin_mut().1.get_pin_mut();
        stream.send(msg).await.context("couldn't send ws message")?;
        Ok(())
    }

    while let Some(input) = inputs.next().await {
        match input? {
            Input::UserEvent(UserEvent::UpdateUser { email }) => {
                let response = WsResponse::Update {
                    email: Some(email),
                    cards: None,
                };
                send_ws(inputs.as_mut(), response).await?;
            }
            Input::UserEvent(UserEvent::DeleteUser) => {
                send_ws(inputs.as_mut(), WsResponse::DeleteUser).await?;
            }
            Input::UserEvent(UserEvent::ChangedCards) => {
                if let Some(opts) = &query_opts {
                    let response = WsResponse::Update {
                        email: None,
                        cards: Some(cards(&database, user_id, opts).await?),
                    };
                    send_ws(inputs.as_mut(), response).await?;
                }
            }
            Input::UserEvent(UserEvent::Lapsed) => {
                if let Some(opts) = &query_opts {
                    send_ws(inputs.as_mut(), query(&database, user_id, opts).await).await?;
                }
            }
            Input::WebSocket(bytes) => {
                let request = match serde_json::from_slice(&bytes) {
                    Ok(req) => req,
                    Err(e) => {
                        let error = WsResponse::Error {
                            message: e.to_string(),
                        };
                        send_ws(inputs.as_mut(), error).await?;
                        continue;
                    }
                };

                match request {
                    WsRequest::SetQueryOpts { opts } => {
                        query_opts = Some(opts);
                        send_ws(inputs.as_mut(), query(&database, user_id, &opts).await).await?;
                    }
                }
            }
            Input::Exit => break,
        }
    }

    Ok(())
}

async fn query(database: &PgPool, user_id: i64, opts: &QueryOpts) -> WsResponse {
    async {
        let mut transaction = database
            .begin()
            .await
            .context("couldn't begin transaction")?;

        let email: String = match sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&mut transaction)
            .await
            .context("failed to query user")?
        {
            Some(email) => email,
            None => return Ok(WsResponse::DeleteUser),
        };

        let cards = cards(&mut transaction, user_id, opts).await?;

        Ok(WsResponse::Update {
            email: Some(Arc::from(email)),
            cards: Some(cards),
        })
    }
    .await
    .unwrap_or_else(|e: anyhow::Error| {
        log::error!("{:?}", e.context("error refreshing data"));
        WsResponse::Error {
            message: "internal server error".to_owned(),
        }
    })
}

async fn cards(
    database: impl PgExecutor<'_>,
    user_id: i64,
    opts: &QueryOpts,
) -> anyhow::Result<Vec<Card>> {
    sqlx::query_as(
        "\
            SELECT id,created_at,terms,definitions,case_sensitive,knowledge,safety_net \
            FROM cards \
            WHERE owner = $1 \
            ORDER BY created_at DESC \
            LIMIT $2 OFFSET $3\
        ",
    )
    .bind(user_id)
    .bind(opts.limit)
    .bind(opts.offset)
    .fetch_all(database)
    .await
    .context("failed to query cards")
}

struct AbortOnDrop<T>(task::JoinHandle<T>);
impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
