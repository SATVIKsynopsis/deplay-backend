use axum::response::sse::{Event, Sse};
use axum::{
    extract::{Json, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::{
    convert::Infallible,
    fs,
    io::{BufRead, BufReader},
    path::Path as FsPath,
    process::{Command, Stdio},
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::cors::{Any, CorsLayer};

use crate::analyze::analyze;
mod analyze;
use crate::detect_dockerfile::general_dockerfile;

mod detect_dockerfile;

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
        .allow_methods(Any)
        .allow_headers([CONTENT_TYPE, AUTHORIZATION, ACCEPT])
        .allow_credentials(false);

    let app = Router::new()
        .route("/run", post(run_repo))
        .route("/logs/:id", get(stream_logs))
        .route("/analysis/:id", get(get_analysis))
        .layer(cors);

    let port = std::env::var("PORT").unwrap_or("8080".to_string());
    let addr = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Server listening on port {}", port);
    axum::serve(listener, app).await.unwrap();
}

async fn run_repo(Json(payload): Json<RunRequest>) -> impl IntoResponse {
    if !payload.repo_url.starts_with("https://github.com/") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Only GitHub repos supported" })),
        );
    }

    let run_id = current_timestamp_millis().to_string();
    let run_id_clone = run_id.clone();
    let repo_url = payload.repo_url.clone();

    let lang = payload.language.clone();

    std::thread::spawn(move || {
        run_job(run_id.clone(), repo_url, lang);
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({ "runId": run_id_clone })),
    )
}

fn run_job(run_id: String, repo_url: String, lang: String) {
    let log_path = format!("/tmp/deplik-{}.log", run_id);
    let clone_root = format!("/tmp/repos/{}", run_id);
    let repo_dir = format!("{}/repo", clone_root);

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

    log("Cloning repository...");
    fs::create_dir_all(&clone_root).unwrap();

    let output = Command::new("git")
        .args(["clone", &repo_url, &repo_dir])
        .output()
        .expect("failed to execute git");

    log("Git clone stdout:");
    log(&String::from_utf8_lossy(&output.stdout));

    log("Git clone stderr:");
    log(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        log("Git clone exited with non-zero status");
        return;
    }

    log(&format!("Generating Dockerfile for language: {}", lang));

    if let Err(e) = detect_dockerfile::general_dockerfile(&repo_dir, &lang) {
        log(&format!("Dockerfile generation failed: {e}"));
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
        return;
    }

    log("Docker build finished");

    let run_id_clone = run_id.clone();

    std::thread::spawn(move || {
        let log_path = format!("/tmp/deplik-{}.log", run_id_clone);

        std::thread::sleep(Duration::from_secs(1));

        let logs = fs::read_to_string(&log_path).unwrap_or_default();

        let rt = tokio::runtime::Runtime::new().unwrap();
        match rt.block_on(analyze(&logs)) {
            Ok(result) => {
                let analysis_path = format!("/tmp/deplik-{}.analysis.json", run_id_clone);
                let json = serde_json::to_string_pretty(&result).unwrap();
                fs::write(&analysis_path, json).unwrap();
            }
           Err(e) => {
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "AI analysis failed: {e}")
        });
}
        }
    });
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
            Json(serde_json::json!({
                "error": "Analysis not ready"
            })),
        ),
    }
}

fn current_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
