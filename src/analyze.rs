use async_openai::{types::responses::CreateResponseArgs, Client};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct AnalyzeResult {
    pub summary: String,
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
}

pub async fn analyze(logs: &str) -> Result<AnalyzeResult, Box<dyn std::error::Error>> {
    let client = Client::new();

    let prompt = format!(
        r#"
You are a senior DevOps engineer.

You are given Docker build and runtime logs from a sandboxed environment.

Your task:
1. Summarize what happened
2. Detect errors, warnings, or misconfigurations
3. Identify environment mismatches (Node versions, missing build steps, ports, etc.)
4. Suggest concrete fixes

Return STRICT JSON ONLY in the following format:
{{
  "summary": "string",
  "issues": ["string"],
  "suggestions": ["string"]
}}

Logs:
{}
"#,
        logs
    );

    let request = CreateResponseArgs::default()
        .model("gpt-5.2")
        .input(prompt)
        .max_output_tokens(700u32)
        .build()?;

    let response = client.responses().create(request).await?;

    let output = response.output_text().ok_or("No output from model")?;

    let analysis: AnalyzeResult = serde_json::from_str(&output)?;

    Ok(analysis)
}
