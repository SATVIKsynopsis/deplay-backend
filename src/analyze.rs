use aws_sdk_bedrockruntime::primitives::Blob;
use aws_config::Region;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct AnalyzeResult {
    pub summary: String,
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
}

pub async fn analyze(
    logs: &str,
) -> Result<AnalyzeResult, Box<dyn std::error::Error + Send + Sync>> {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .load()
        .await;
    let client = aws_sdk_bedrockruntime::Client::new(&config);

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

    let body = serde_json::json!({
        "anthropic_version": "bedrock-2023-05-31",
        "max_tokens": 1000,
        "messages": [
            {
                "role": "user",
                "content": prompt
            }
        ]
    });

   let response = client
        .invoke_model()
        .model_id("anthropic.claude-3-5-haiku-20241022-v1:0")
        .content_type("application/json")
        .body(Blob::new(serde_json::to_vec(&body)?))
        .send()
        .await
        .map_err(|e| {
            eprintln!("Bedrock full error: {:?}", e);
            e
        })?;

    let response_body: serde_json::Value =
        serde_json::from_slice(response.body().as_ref())?;

    let output = response_body["content"][0]["text"]
        .as_str()
        .ok_or("No text in response")?;

    let clean = output
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let analysis: AnalyzeResult = serde_json::from_str(clean)?;

    Ok(analysis)
}