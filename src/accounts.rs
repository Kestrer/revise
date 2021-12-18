use crate::{
    event,
    session::Session,
    utils::{EndpointError, EndpointResult, NonEmptyString, ReqTransaction},
};
use anyhow::Context;
use axum::{
    extract::Form,
    http::{StatusCode, Uri},
    response::{IntoResponse, Redirect},
    routing, Json, Router,
};
use serde::Deserialize;

pub(crate) fn routes() -> Router {
    Router::new()
        .route("/login", routing::post(login))
        .route("/logout", routing::post(logout))
        // weaker form of logout, where the session is known to be ended (e.g. the account has been
        // deleted)
        .route("/clear-session-cookie", routing::get(clear_session_cookie))
        .route("/create", routing::post(create))
        .route("/delete", routing::post(delete))
        .route("/me/email", routing::put(set_email))
        .route("/me/password", routing::put(set_password))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogIn {
    email: String,
    password: String,
}

async fn login(form: Form<LogIn>, mut transaction: ReqTransaction) -> EndpointResult {
    let user_id: i64 = sqlx::query_scalar(
        "SELECT id FROM users WHERE email = $1 AND password = crypt($2, password)",
    )
    .bind(&form.email)
    .bind(&form.password)
    .fetch_optional(&mut *transaction)
    .await
    .context("failed to check password correctness")?
    .ok_or_else(|| Redirect::to(Uri::from_static("/?loginError=")).into_response())?;

    let session = Session::new();

    sqlx::query("INSERT INTO session_cookies VALUES ($1, $2)")
        .bind(&session)
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .context("failed to insert new session cookie")?;

    transaction.commit().await?;

    Ok(session.set_cookie_on(Redirect::to(Uri::from_static("/"))))
}

async fn logout(session: Session, mut transaction: ReqTransaction) -> EndpointResult {
    sqlx::query("DELETE FROM session_cookies WHERE cookie_value = $1")
        .bind(&session)
        .execute(&mut *transaction)
        .await
        .context("failed to log out")?;

    event::Notify::LogOut { session: &session }
        .broadcast(&mut *transaction)
        .await?;

    transaction.commit().await?;

    clear_session_cookie().await
}

async fn clear_session_cookie() -> EndpointResult {
    Ok(Session::clear_cookie_on(Redirect::to(Uri::from_static(
        "/",
    ))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateAccount {
    email: NonEmptyString,
    password: NonEmptyString,
}

async fn create(form: Form<CreateAccount>, mut transaction: ReqTransaction) -> EndpointResult {
    let user_id: i64 = sqlx::query_scalar(
        "\
            INSERT INTO users VALUES (DEFAULT, $1, crypt($2, gen_salt('bf', 8))) \
            ON CONFLICT DO NOTHING \
            RETURNING id\
        ",
    )
    .bind(&form.email)
    .bind(&form.password)
    .fetch_optional(&mut *transaction)
    .await
    .context("failed to add new user")?
    .ok_or_else(|| Redirect::to(Uri::from_static("/?createAccountError=")).into_response())?;

    let session = Session::new();

    sqlx::query("INSERT INTO session_cookies VALUES ($1, $2)")
        .bind(&session)
        .bind(&user_id)
        .execute(&mut *transaction)
        .await
        .context("failed to insert new session cookie")?;

    transaction.commit().await?;

    Ok(session.set_cookie_on(Redirect::to(Uri::from_static("/"))))
}

async fn delete(session: Session, mut transaction: ReqTransaction) -> EndpointResult {
    let user_id = session.user_id_http(&mut *transaction).await?;

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .context("failed to delete account")?;

    event::Notify::DeleteUser { id: user_id }
        .broadcast(&mut *transaction)
        .await?;

    transaction.commit().await?;

    Ok(Session::clear_cookie_on(Redirect::to(Uri::from_static(
        "/",
    ))))
}

async fn set_email(
    session: Session,
    body: Json<NonEmptyString>,
    mut transaction: ReqTransaction,
) -> EndpointResult {
    let user_id = session.user_id_http(&mut *transaction).await?;

    let res = sqlx::query("UPDATE users SET email = $1 WHERE id = $2")
        .bind(&*body)
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .context("couldn't set user email")?;

    if res.rows_affected() == 0 {
        return Err(EndpointError::new((
            StatusCode::NOT_FOUND,
            "account deleted",
        )));
    }

    event::Notify::UpdateUser { id: user_id, email: &body }
        .broadcast(&mut *transaction)
        .await?;

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetPassword {
    old_password: NonEmptyString,
    new_password: NonEmptyString,
}

async fn set_password(
    session: Session,
    body: Json<SetPassword>,
    mut transaction: ReqTransaction,
) -> EndpointResult {
    let user_id = session.user_id_http(&mut *transaction).await?;

    let res = sqlx::query(
        "\
            UPDATE users \
                SET password = crypt($1, gen_salt('bf', 8)) \
                WHERE id = $2 AND password = crypt($3, password)\
        "
    )
    .bind(&*body.new_password)
    .bind(user_id)
    .bind(&*body.old_password)
    .execute(&mut *transaction)
    .await
    .context("couldn't set password")?;

    if res.rows_affected() == 0 {
        return Err(EndpointError::new((
            StatusCode::UNAUTHORIZED,
            "old password incorrect",
        )));
    }

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
