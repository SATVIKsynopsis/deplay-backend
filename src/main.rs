use axum::{
    extract::Json,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunRequest {
    repo_url: String,
}

#[derive(Serialize)]
struct AnalysisResponse {
    summary: String,
    suggestions: Vec<String>,
}

#[derive(Serialize)]
struct RunResponse {
    status: String,
    logs: String,
    analysis: AnalysisResponse,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/run", post(run_repo));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    println!("Server listening on 3001");
    axum::serve(listener, app).await.unwrap();
}

async fn run_repo(Json(payload): Json<RunRequest>) -> Response {
    if !payload.repo_url.starts_with("https://github.com/") {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "repoUrl must start with https://github.com/".to_string(),
            }),
        )
            .into_response();
    }

    let public_check = Command::new("git")
        .arg("ls-remote")
        .arg(&payload.repo_url)
        .output()
        .unwrap();

    if !public_check.status.success() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Only public repositories are supported".to_string(),
            }),
        )
            .into_response();
    }

    let timestamp = current_timestamp_millis();
    let clone_root = format!("/tmp/repos/{}", timestamp);
    let repo_dir = format!("{}/repo", clone_root);

    fs::create_dir_all(&clone_root).unwrap();

    let clone_output = Command::new("git")
        .arg("clone")
        .arg(&payload.repo_url)
        .arg(&repo_dir)
        .output()
        .unwrap();

    if !clone_output.status.success() {
        let logs = format!(
            "git clone failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&clone_output.stdout),
            String::from_utf8_lossy(&clone_output.stderr)
        );

        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: logs,
            }),
        )
            .into_response();
    }

    ensure_dockerfile_exists(&repo_dir).unwrap();

    let build_output = Command::new("docker")
        .current_dir(&repo_dir)
        .arg("build")
        .arg("-t")
        .arg("depliksandbox")
        .arg(".")
        .output()
        .unwrap();

    let run_output = Command::new("docker")
        .arg("run")
        .arg("--rm")
        .arg("depliksandbox")
        .output()
        .unwrap();

    let logs = format!(
        "=== docker build ===\nstdout:\n{}\nstderr:\n{}\n\n=== docker run ===\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr),
        String::from_utf8_lossy(&run_output.stdout),
        String::from_utf8_lossy(&run_output.stderr)
    );

    let ai_stub = analyze_logs(logs.clone());

    let response = RunResponse {
        status: ai_stub.status,
        logs,
        analysis: AnalysisResponse {
            summary: ai_stub.summary,
            suggestions: ai_stub.suggestions,
        },
    };

    (StatusCode::OK, Json(response)).into_response()
}

fn ensure_dockerfile_exists(repo_dir: &str) -> std::io::Result<()> {
    let dockerfile_path = Path::new(repo_dir).join("Dockerfile");

    if !dockerfile_path.exists() {
        let default_dockerfile = r#"FROM node:18
WORKDIR /app
COPY . .
RUN npm install
CMD [\"npm\", \"start\"]
"#;
        fs::write(dockerfile_path, default_dockerfile)?;
    }

    Ok(())
}

struct AiStubResult {
    status: String,
    summary: String,
    suggestions: Vec<String>,
}

fn analyze_logs(_logs: String) -> AiStubResult {
    AiStubResult {
        status: "NOT_READY".to_string(),
        summary: "Application failed due to missing environment variables".to_string(),
        suggestions: vec![
            "Add DATABASE_URL environment variable".to_string(),
            "Ensure correct PORT is exposed".to_string(),
        ],
    }
}

fn current_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
