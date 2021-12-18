//! Websocket endpoint that gives a stream of user events.

use crate::{event, session::Session, utils::EndpointResult};
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
use serde::{Deserialize, Serialize};
use sqlx::{PgExecutor, PgPool};
use std::{pin::Pin, sync::Arc};
use tokio::{pin, sync::broadcast, task};

pub(crate) async fn routes(database: PgPool) -> anyhow::Result<Router> {
    let (events, _) = broadcast::channel(64);

    let event_stream = event::subscribe(database.clone()).await?;
    let task_sender = events.clone();
    let task = Arc::new(AbortOnDrop(task::spawn(async move {
        pin!(event_stream);
        while let Some(event) = event_stream.next().await {
            let _ = task_sender.send(event);
        }
    })));

    Ok(Router::new().route(
        "/",
        routing::get(|session: Session, ws: WebSocketUpgrade| async move {
            let _task = task;
            let receiver = events.subscribe();
            EndpointResult::Ok(ws.on_upgrade(move |ws| async move {
                if let Err(e) = handle_ws(database, session, receiver, ws).await {
                    log::error!("ws stream ended with error: {:?}", anyhow!(e));
                }
            }))
        }),
    ))
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
    LogOut,
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
    user_session: Session,
    mut events: broadcast::Receiver<event::Received>,
    mut ws: WebSocket,
) -> anyhow::Result<()> {
    let user_id = match user_session.user_id(&database).await? {
        Some(user_id) => user_id,
        None => {
            send_to_ws(&mut ws, WsResponse::LogOut).await?;
            return Ok(());
        }
    };

    enum Input {
        Event(event::Received),
        WebSocket(Vec<u8>),
        Exit,
    }

    let mut query_opts = None;

    let inputs = stream::select(
        stream! {
            loop {
                match events.recv().await {
                    Ok(event) => yield Ok(Input::Event(event)),
                    Err(broadcast::error::RecvError::Closed) => {
                        yield Ok(Input::Exit);
                        break;
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        yield Ok(Input::Event(event::Received::Lapsed));
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
        send_to_ws(&mut stream.as_mut().get_pin_mut().1.get_pin_mut(), msg).await
    }

    while let Some(input) = inputs.next().await {
        match input? {
            Input::Event(event::Received::UpdateUser { id, email }) => {
                if id == user_id {
                    let response = WsResponse::Update {
                        email: Some(email),
                        cards: None,
                    };
                    send_ws(inputs.as_mut(), response).await?;
                }
            }
            Input::Event(event::Received::DeleteUser { id }) => {
                if id == user_id {
                    send_ws(inputs.as_mut(), WsResponse::LogOut).await?;
                }
            }
            Input::Event(event::Received::LogOut { session }) => {
                if session == user_session {
                    send_ws(inputs.as_mut(), WsResponse::LogOut).await?;
                }
            }
            Input::Event(event::Received::ChangedCards { for_user }) => {
                if for_user == user_id {
                    if let Some(opts) = &query_opts {
                        let response = WsResponse::Update {
                            email: None,
                            cards: Some(cards(&database, user_id, opts).await?),
                        };
                        send_ws(inputs.as_mut(), response).await?;
                    }
                }
            }
            Input::Event(event::Received::Lapsed) => {
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

async fn send_to_ws(ws: &mut WebSocket, msg: WsResponse) -> anyhow::Result<()> {
    let json = serde_json::to_string(&msg).expect("serializing ws response as json failed");
    let msg = ws::Message::Text(json);
    ws.send(msg).await.context("couldn't send ws message")?;
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
            None => return Ok(WsResponse::LogOut),
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
