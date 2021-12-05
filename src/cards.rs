use crate::{
    session::Session,
    utils::{EndpointError, EndpointResult, NonBlankString, ReqTransaction},
};
use anyhow::Context as _;
use axum::{extract::Path, http::StatusCode, response::IntoResponse, routing, Json, Router};
use serde::{Deserialize, Serialize};

pub(crate) fn routes() -> Router {
    Router::new()
        .route("/", routing::get(get).post(create))
        .route("/:id", routing::put(modify).delete(delete))
}

#[derive(sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
struct Card {
    id: i64,
    created_at: i64,
    terms: String,
    definitions: String,
    case_sensitive: bool,
    knowledge: i16,
    safety_net: bool,
}

async fn get(session: Session, mut transaction: ReqTransaction) -> EndpointResult {
    let user_id = session.user_id(&mut *transaction).await?;

    let cards: Vec<Card> = sqlx::query_as(
        "\
            SELECT id,created_at,terms,definitions,case_sensitive,knowledge,safety_net \
            FROM cards \
            WHERE owner = $1 \
            ORDER BY created_at DESC\
        ",
    )
    .bind(user_id)
    .fetch_all(&mut *transaction)
    .await
    .context("failed to query cards")?;

    Ok(Json(cards).into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
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
    let user_id = session.user_id(&mut *transaction).await?;

    sqlx::query("INSERT INTO cards VALUES (DEFAULT, $1, $2, $3, $4, $5)")
        .bind(user_id)
        .bind(&body.created_at)
        .bind(&body.terms)
        .bind(&body.definitions)
        .bind(&body.case_sensitive)
        .execute(&mut *transaction)
        .await
        .context("failed to insert card")?;

    transaction.commit().await?;

    Ok(StatusCode::CREATED.into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
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
    let user_id = session.user_id(&mut *transaction).await?;

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

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn delete(
    id: Path<i64>,
    session: Session,
    mut transaction: ReqTransaction,
) -> EndpointResult {
    let user_id = session.user_id(&mut *transaction).await?;

    let res = sqlx::query("DELETE FROM cards WHERE id = $1 AND OWNER = $2")
        .bind(*id)
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .context("couldn't delete card")?;

    if res.rows_affected() == 0 {
        return Err(EndpointError::new(StatusCode::NOT_FOUND));
    }

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
