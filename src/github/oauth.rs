use crate::github::models::GithubUser;

use reqwest::Client;
use serde_json::Value;

pub async fn exchange_code(code: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = Client::new();

    let res: Value = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": std::env::var("GITHUB_CLIENT_ID")?,
            "client_secret": std::env::var("GITHUB_CLIENT_SECRET")?,
            "code": code,
        }))
        .send()
        .await?
        .json()
        .await?;

    let access_token = match res.get("access_token").and_then(|v| v.as_str()) {
        Some(token) => token.to_string(),
        None => {
            eprintln!("GitHub OAuth error response: {:?}", res);
            return Err("GitHub OAuth failed: no access_token".into());
        }
    };

    Ok(access_token)
}

pub async fn fetch_user(token: &str) -> GithubUser {
    let client = reqwest::Client::new();

    client
        .get("https://api.github.com/user")
        .header("User-Agent", "deplik-backend")
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json::<GithubUser>()
        .await
        .unwrap()
}
