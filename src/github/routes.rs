use crate::db;
use crate::db::user::upsert_user;
use axum::response::Response;
use axum::{
    extract::Query,
    http::{header::SET_COOKIE, HeaderMap},
    response::{IntoResponse, Redirect},
};
use cookie::{Cookie, SameSite};
use http::StatusCode;
use time::Duration;

use crate::github::{
    models::GithubCallback,
    oauth::{exchange_code, fetch_user},
};

pub async fn github_login() -> Redirect {
    let client_id = std::env::var("GITHUB_CLIENT_ID").unwrap();
    let redirect_uri = format!("{}/auth/github/callback", std::env::var("APP_URL").unwrap());

    let url = format!(
        "https://github.com/login/oauth/authorize\
        ?client_id={}&redirect_uri={}&scope=user:email",
        client_id, redirect_uri
    );

    Redirect::to(&url)
}

pub async fn github_callback(Query(query): Query<GithubCallback>) -> Result<Response, StatusCode> {
    let token = exchange_code(&query.code).await.map_err(|e| {
        eprintln!("OAuth error: {e}");
        StatusCode::UNAUTHORIZED
    })?;

    let user = fetch_user(&token).await;

    let dynamo = db::dynamo_client().await;
    upsert_user(&dynamo, user.id, &user.login, &user.avatar_url, &token).await;

    let cookie = Cookie::build(("session", user.id.to_string()))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(false)
        .path("/")
        .max_age(Duration::days(7))
        .build();

    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.to_string().parse().unwrap());

    Ok((
    headers,
    Redirect::to(&format!("{}/dashboard", std::env::var("APP_URL").unwrap())),
)
    .into_response())
}
