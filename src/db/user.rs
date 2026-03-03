use anyhow::Result;
use aws_sdk_dynamodb::{types::AttributeValue, Client};
use chrono::Utc;

pub async fn save_user(client: &Client, github_id: i64, username: &str, access_token: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    let pk = format!("USER#github_{}", github_id);

    client
        .put_item()
        .table_name("Deplay")
        .item("pk", AttributeValue::S(pk))
        .item("sk", AttributeValue::S("PROFILE".to_string()))
        .item("provider", AttributeValue::S("github".to_string()))
        .item("accessToken", AttributeValue::S(access_token.to_string()))
        .item("githubId", AttributeValue::N(github_id.to_string()))
        .item("username", AttributeValue::S(username.to_string()))
        .send()
        .await
        .unwrap();
}

pub async fn upsert_user(
    client: &Client,
    github_id: i64,
    username: &str,
    avatar_url: &str,
    access_token: &str,
) {
    let now = chrono::Utc::now().to_rfc3339();

    let pk = format!("USER#github_{}", github_id);

    client
        .put_item()
        .table_name("Deplay")
        .item("pk", AttributeValue::S(pk))
        .item("sk", AttributeValue::S("PROFILE".to_string()))
        .item("provider", AttributeValue::S("github".to_string()))
        .item("githubId", AttributeValue::N(github_id.to_string()))
        .item("username", AttributeValue::S(username.to_string()))
        .item("avatarUrl", AttributeValue::S(avatar_url.to_string()))
        .item("createdAt", AttributeValue::S(now.clone()))
        .item("lastLogin", AttributeValue::S(now))
        .item("accessToken", AttributeValue::S(access_token.to_string()))
        .send()
        .await
        .unwrap();
}

pub async fn get_user(client: &Client, pk: &str) -> Result<serde_json::Value> {
    let res = client
        .get_item()
        .table_name("Deplay")
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S("PROFILE".to_string()))
        .send()
        .await?;

    let item = res.item.ok_or_else(|| anyhow::anyhow!("User not found"))?;

    Ok(serde_json::json!({
        "githubId": item["githubId"].as_n().unwrap(),
        "username": item["username"].as_s().unwrap(),
        "avatarUrl": item["avatarUrl"].as_s().unwrap(),
        "provider": item["provider"].as_s().unwrap(),
        "createdAt": item["createdAt"].as_s().unwrap(),
        "lastLogin": item["lastLogin"].as_s().unwrap(),
        "accessToken": item["accessToken"].as_s().unwrap()
    }))
}
