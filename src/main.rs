use axum::response::sse::{Event, Sse};
use axum::{
    extract::{Json, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunRequest {
    repo_url: String,
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/run", post(run_repo))
        .route("/logs/:id", get(stream_logs));

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

    std::thread::spawn(move || {
        run_job(run_id.clone(), repo_url);
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({ "runId": run_id_clone })),
    )
}

fn run_job(run_id: String, repo_url: String) {
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

    ensure_dockerfile_exists(&repo_dir).unwrap();

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

    let _ = child.wait();
    log("Docker build finished");
    let _ = fs::remove_dir_all(&clone_root);
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
                    yield Ok(Event::default().data(new.to_string()));
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };

    Sse::new(stream)
}

fn ensure_dockerfile_exists(repo_dir: &str) -> std::io::Result<()> {
    let dockerfile_path = FsPath::new(repo_dir).join("Dockerfile");

    if !dockerfile_path.exists() {
        let dockerfile = r#"FROM node:20
WORKDIR /app
COPY . .
RUN npm install
CMD ["npm", "start"]
"#;
        fs::write(dockerfile_path, dockerfile)?;
    }

    Ok(())
}

fn current_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
