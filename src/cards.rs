use crate::{
    event,
    session::Session,
    utils::{EndpointError, EndpointResult, NonBlankString, ReqTransaction},
};
use anyhow::Context as _;
use axum::{extract::Path, http::StatusCode, response::IntoResponse, routing, Json, Router};
use serde::Deserialize;

pub(crate) fn routes() -> Router {
    Router::new()
        .route("/", routing::post(create))
        .route("/:id", routing::put(modify).delete(delete))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateCard {
    created_at: i64,
    terms: NonBlankString,
    definitions: NonBlankString,
    case_sensitive: bool,
}

async fn create(
    session: Session,
    body: Json<CreateCard>,
    mut transaction: ReqTransaction,
) -> EndpointResult {
    let user_id = session.user_id_http(&mut *transaction).await?;

    sqlx::query("INSERT INTO cards VALUES (DEFAULT, $1, $2, $3, $4, $5)")
        .bind(user_id)
        .bind(&body.created_at)
        .bind(&body.terms)
        .bind(&body.definitions)
        .bind(&body.case_sensitive)
        .execute(&mut *transaction)
        .await
        .context("failed to insert card")?;

    notify_card_change(&mut *transaction, user_id).await?;

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModifyCard {
    terms: Option<NonBlankString>,
    definitions: Option<NonBlankString>,
    case_sensitive: Option<bool>,
}

async fn modify(
    id: Path<i64>,
    session: Session,
    body: Json<ModifyCard>,
    mut transaction: ReqTransaction,
) -> EndpointResult {
    let user_id = session.user_id_http(&mut *transaction).await?;

    let res = sqlx::query(
        "\
            UPDATE cards \
            SET \
                terms = COALESCE($1, terms),\
                definitions = COALESCE($2, definitions),\
                case_sensitive = COALESCE($3, case_sensitive) \
            WHERE \
                id = $4 AND owner = $5\
        ",
    )
    .bind(&body.terms)
    .bind(&body.definitions)
    .bind(&body.case_sensitive)
    .bind(*id)
    .bind(user_id)
    .execute(&mut *transaction)
    .await
    .context("couldn't modify card")?;

    if res.rows_affected() == 0 {
        return Err(EndpointError::new(StatusCode::NOT_FOUND));
    }

    notify_card_change(&mut *transaction, user_id).await?;

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn delete(
    id: Path<i64>,
    session: Session,
    mut transaction: ReqTransaction,
) -> EndpointResult {
    let user_id = session.user_id_http(&mut *transaction).await?;

    let res = sqlx::query("DELETE FROM cards WHERE id = $1 AND OWNER = $2")
        .bind(*id)
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .context("couldn't delete card")?;

    if res.rows_affected() == 0 {
        return Err(EndpointError::new(StatusCode::NOT_FOUND));
    }

    notify_card_change(&mut *transaction, user_id).await?;

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn notify_card_change(
    database: impl sqlx::PgExecutor<'_>,
    for_user: i64,
) -> anyhow::Result<()> {
    event::Notify::ChangedCards { for_user }
        .broadcast(database)
        .await
}
