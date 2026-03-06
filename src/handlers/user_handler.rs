use aws_sdk_dynamodb::types::AttributeValue;
use axum::http::{header::SET_COOKIE, HeaderMap};
use axum::response::Redirect;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use time::Duration;

use crate::db;
use crate::db::user::get_user;

pub async fn me(cookies: CookieJar) -> impl IntoResponse {
    let github_id = match cookies.get("session") {
        Some(c) => c.value().to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Not logged in" })),
            );
        }
    };

    let pk = format!("USER#github_{}", github_id);

    let client = db::dynamo_client().await;

    match get_user(&client, &pk).await {
        Ok(user) => (StatusCode::OK, Json(user)),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "User not found" })),
        ),
    }
}

pub async fn get_repos(cookies: CookieJar) -> impl IntoResponse {
    let github_id = match cookies.get("session") {
        Some(c) => c.value().to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Not logged in" })),
            )
        }
    };

    let pk = format!("USER#github_{}", github_id);
    let client = db::dynamo_client().await;

    let user = match get_user(&client, &pk).await {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "User not found" })),
            )
        }
    };

    let token = match user["accessToken"].as_str() {
        Some(t) => t.to_string(),
        None => {
            eprintln!("No accessToken found for user: {:?}", user);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "No access token stored for this user" })),
            );
        }
    };

    let http = reqwest::Client::new();
    let repos = http
        .get("https://api.github.com/user/repos?sort=updated&per_page=100")
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "deplay-app")
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    (StatusCode::OK, Json(repos))
}

pub async fn logout(jar: CookieJar) -> impl IntoResponse {
    let session = Cookie::build(("session", ""))
        .path("/")
        .max_age(Duration::seconds(0))
        .build();

    let gh_token = Cookie::build(("gh_token", ""))
        .path("/")
        .max_age(Duration::seconds(0))
        .build();

    let mut headers = HeaderMap::new();
    headers.append(SET_COOKIE, session.to_string().parse().unwrap());
    headers.append(SET_COOKIE, gh_token.to_string().parse().unwrap());

    (headers, Redirect::to("https://deplay-theta.vercel.app")).into_response()
}

pub async fn get_runs(cookies: CookieJar) -> impl IntoResponse {
    let github_id = match cookies.get("session") {
        Some(c) => c.value().to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Not logged in" })),
            )
        }
    };

    let pk = format!("USER#github_{}", github_id);
    let client = db::dynamo_client().await;

    let res = client
        .query()
        .table_name("Deplay")
        .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
        .expression_attribute_values(":pk", AttributeValue::S(pk))
        .expression_attribute_values(":prefix", AttributeValue::S("RUN#".to_string()))
        .scan_index_forward(false)
        .send()
        .await;

    match res {
        Ok(output) => {
            let items: Vec<serde_json::Value> = output.items().iter().map(|item| {
    serde_json::json!({
        "runId": item.get("runId").and_then(|v| v.as_s().ok()).map(|s| s.as_str()).unwrap_or(""),
        "repoName": item.get("repoName").and_then(|v| v.as_s().ok()).map(|s| s.as_str()).unwrap_or(""),
        "repoUrl": item.get("repoUrl").and_then(|v| v.as_s().ok()).map(|s| s.as_str()).unwrap_or(""),
        "language": item.get("language").and_then(|v| v.as_s().ok()).map(|s| s.as_str()).unwrap_or(""),
        "status": item.get("status").and_then(|v| v.as_s().ok()).map(|s| s.as_str()).unwrap_or(""),
        "createdAt": item.get("createdAt").and_then(|v| v.as_s().ok()).map(|s| s.as_str()).unwrap_or(""),
        "logsS3Key": item.get("logsS3Key").and_then(|v| v.as_s().ok()).map(|s| s.as_str()).unwrap_or(""),
        "analysisS3Key": item.get("analysisS3Key").and_then(|v| v.as_s().ok()).map(|s| s.as_str()).unwrap_or(""),
    })
}).collect();

            (StatusCode::OK, Json(serde_json::json!(items)))
        }
        Err(e) => {
            eprintln!("DynamoDB query failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to fetch runs" })),
            )
        }
    }
}
