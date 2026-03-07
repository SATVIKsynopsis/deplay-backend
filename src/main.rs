use aws_sdk_dynamodb::types::AttributeValue;
use axum::http::{header::SET_COOKIE, HeaderMap};
use axum::response::sse::{Event, Sse};
use axum::{
    extract::{Json, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use http::header::HeaderValue;
use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use http::Method;
use serde::{Deserialize, Serialize};
use std::{
    convert::Infallible,
    fs,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower_http::cors::{Any, CorsLayer};

use crate::analyze::analyze;
mod analyze;
mod db;
mod handlers;
use crate::detect_dockerfile::general_dockerfile;
use crate::handlers::user_handler;
mod detect_dockerfile;
mod github;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunRequest {
    repo_url: String,
    language: String,
}

#[derive(Serialize)]
struct Issue {
    kind: String,
    message: String,
    suggestion: String,
}

#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin(["http://localhost:3000".parse().unwrap()])
        .allow_origin(
            "https://deplay-theta.vercel.app"
                .parse::<HeaderValue>()
                .unwrap(),
        )
        .allow_origin(
            "https://main.dsb7vt97yonc2.amplifyapp.com"
                .parse::<HeaderValue>()
                .unwrap(),
        )
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION, ACCEPT])
        .allow_credentials(true);

    let app = Router::new()
        .route("/run", post(run_repo))
        .route("/logs/:id", get(stream_logs))
        .route("/analysis/:id", get(get_analysis))
        .route("/auth/github", get(github::github_login))
        .route("/auth/github/callback", get(github::github_callback))
        .route("/me", get(user_handler::me))
        .route("/logout", get(user_handler::logout))
        .route("/repos", get(user_handler::get_repos))
        .route("/runs", get(user_handler::get_runs))
        .route("/logs-static/:id", get(get_logs_static))
        .layer(cors);

    let port = std::env::var("PORT").unwrap_or("8080".to_string());
    let addr = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Server listening on port {}", port);
    axum::serve(listener, app).await.unwrap();
}

async fn run_repo(cookies: CookieJar, Json(payload): Json<RunRequest>) -> impl IntoResponse {
    if !payload.repo_url.starts_with("https://github.com/") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Only GitHub repos supported" })),
        );
    }

    let user_id = match cookies.get("session") {
        Some(c) => c.value().to_string(),
        None => "anonymous".to_string(),
    };

    let run_id = current_timestamp_millis().to_string();
    let repo_url = payload.repo_url.clone();
    let lang = payload.language.clone();

    tokio::spawn(run_job(run_id.clone(), repo_url, lang, user_id));

    (StatusCode::OK, Json(serde_json::json!({ "runId": run_id })))
}

async fn run_job(run_id: String, repo_url: String, lang: String, user_id: String) {
    let log_path = format!("/tmp/deplik-{}.log", run_id);
    let clone_root = format!("/tmp/repos/{}", run_id);
    let repo_dir = format!("{}/repo", clone_root);

    let repo_name = repo_url
        .trim_end_matches('/')
        .trim_start_matches("https://github.com/")
        .to_string();

    let mut log = |msg: &str| {
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{msg}")
            });
    };

    let dynamo = db::dynamo_client().await;
    let created_at = chrono::Utc::now().to_rfc3339();
    let _ = dynamo
        .put_item()
        .table_name("Deplay")
        .item("pk", AttributeValue::S(format!("USER#github_{}", user_id)))
        .item("sk", AttributeValue::S(format!("RUN#{}", run_id)))
        .item("runId", AttributeValue::S(run_id.clone()))
        .item("repoUrl", AttributeValue::S(repo_url.clone()))
        .item("repoName", AttributeValue::S(repo_name.clone()))
        .item("language", AttributeValue::S(lang.clone()))
        .item("status", AttributeValue::S("PENDING".to_string()))
        .item("createdAt", AttributeValue::S(created_at))
        .send()
        .await;

    log("Cloning repository...");
    let _ = fs::create_dir_all(&clone_root);

    let output = Command::new("git")
        .args(["clone", &repo_url, &repo_dir])
        .output()
        .unwrap();

    log(&String::from_utf8_lossy(&output.stdout));
    log(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        log("Git clone exited with non-zero status");
        update_run_status(&dynamo, &user_id, &run_id, "FAILED").await;
        return;
    }

    log(&format!("Generating Dockerfile for language: {}", lang));
    if let Err(e) = general_dockerfile(&repo_dir, &lang) {
        log(&format!("Dockerfile generation failed: {e}"));
        update_run_status(&dynamo, &user_id, &run_id, "FAILED").await;
        return;
    }

    log("Building Docker image...");
    let mut child = Command::new("docker")
        .current_dir(&repo_dir)
        .args(["build", "-t", "depliksandbox", "."])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().flatten() {
            log(&line);
        }
    }

    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().flatten() {
            log(&line);
        }
    }

    let status = child.wait().unwrap();
    if !status.success() {
        log("Docker build failed");
        let logs_content = fs::read_to_string(&log_path).unwrap_or_default();
        let logs_s3_key = format!("runs/{}/logs.txt", run_id);
        upload_to_s3(&logs_s3_key, logs_content.into_bytes()).await;
        update_run_status_with_logs(&dynamo, &user_id, &run_id, "FAILED", &logs_s3_key).await;
        return;
    }

    log("Docker build finished");

    let log_path_clone = log_path.clone();
    let run_id_clone = run_id.clone();
    let user_id_clone = user_id.clone();

    tokio::spawn(async move {
        let logs = fs::read_to_string(&log_path_clone).unwrap_or_default();

        let logs_s3_key = format!("runs/{}/logs.txt", run_id_clone);
        upload_to_s3(&logs_s3_key, logs.clone().into_bytes()).await;

        match analyze(&logs).await {
            Ok(result) => {
                let analysis_path = format!("/tmp/deplik-{}.analysis.json", run_id_clone);
                let json = serde_json::to_string_pretty(&result).unwrap();
                let _ = fs::write(&analysis_path, &json);

                let analysis_s3_key = format!("runs/{}/analysis.json", run_id_clone);
                upload_to_s3(&analysis_s3_key, json.into_bytes()).await;

                let dynamo = db::dynamo_client().await;
                let _ = dynamo
                    .update_item()
                    .table_name("Deplay")
                    .key(
                        "pk",
                        AttributeValue::S(format!("USER#github_{}", user_id_clone)),
                    )
                    .key("sk", AttributeValue::S(format!("RUN#{}", run_id_clone)))
                    .update_expression("SET #s = :s, logsS3Key = :l, analysisS3Key = :a")
                    .expression_attribute_names("#s", "status")
                    .expression_attribute_values(":s", AttributeValue::S("SUCCESS".to_string()))
                    .expression_attribute_values(
                        ":l",
                        AttributeValue::S(format!("runs/{}/logs.txt", run_id_clone)),
                    )
                    .expression_attribute_values(
                        ":a",
                        AttributeValue::S(format!("runs/{}/analysis.json", run_id_clone)),
                    )
                    .send()
                    .await;
            }
            Err(e) => {
                eprintln!("AI analysis failed: {e}");
                let dynamo = db::dynamo_client().await;
                update_run_status(&dynamo, &user_id_clone, &run_id_clone, "FAILED").await;
            }
        }
    });
}

async fn update_run_status(
    dynamo: &aws_sdk_dynamodb::Client,
    user_id: &str,
    run_id: &str,
    status: &str,
) {
    let _ = dynamo
        .update_item()
        .table_name("Deplay")
        .key("pk", AttributeValue::S(format!("USER#github_{}", user_id)))
        .key("sk", AttributeValue::S(format!("RUN#{}", run_id)))
        .update_expression("SET #s = :s")
        .expression_attribute_names("#s", "status")
        .expression_attribute_values(":s", AttributeValue::S(status.to_string()))
        .send()
        .await;
}

async fn update_run_status_with_logs(
    dynamo: &aws_sdk_dynamodb::Client,
    user_id: &str,
    run_id: &str,
    status: &str,
    logs_key: &str,
) {
    let _ = dynamo
        .update_item()
        .table_name("Deplay")
        .key("pk", AttributeValue::S(format!("USER#github_{}", user_id)))
        .key("sk", AttributeValue::S(format!("RUN#{}", run_id)))
        .update_expression("SET #s = :s, logsS3Key = :l")
        .expression_attribute_names("#s", "status")
        .expression_attribute_values(":s", AttributeValue::S(status.to_string()))
        .expression_attribute_values(":l", AttributeValue::S(logs_key.to_string()))
        .send()
        .await;
}

async fn upload_to_s3(key: &str, data: Vec<u8>) {
    let config = aws_config::load_from_env().await;
    let s3 = aws_sdk_s3::Client::new(&config);
    if let Err(e) = s3
        .put_object()
        .bucket("deplik-runs")
        .key(key)
        .body(data.into())
        .send()
        .await
    {
        eprintln!("S3 upload failed for key {}: {e}", key);
    }
}

async fn stream_logs(
    Path(run_id): Path<String>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let log_path = format!("/tmp/deplik-{}.log", run_id);

    let stream = async_stream::stream! {
        let mut last_len = 0;
        loop {
            if let Ok(content) = fs::read_to_string(&log_path) {
                if content.len() > last_len {
                    let new = &content[last_len..];
                    last_len = content.len();
                    for line in new.lines() {
                        let clean = line.trim_end_matches('\r');
                        if !clean.is_empty() {
                            yield Ok(Event::default().data(clean.to_string()));
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

async fn get_analysis(Path(run_id): Path<String>) -> impl IntoResponse {
    let path = format!("/tmp/deplik-{}.analysis.json", run_id);

    match fs::read_to_string(path) {
        Ok(content) => (
            StatusCode::OK,
            Json(serde_json::from_str::<serde_json::Value>(&content).unwrap()),
        ),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Analysis not ready" })),
        ),
    }
}

fn current_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

async fn get_logs_static(Path(run_id): Path<String>) -> impl IntoResponse {
    let path = format!("/tmp/deplik-{}.log", run_id);
    match fs::read_to_string(path) {
        Ok(content) => (StatusCode::OK, content),
        Err(_) => (StatusCode::NOT_FOUND, "Logs not found".to_string()),
    }
}
