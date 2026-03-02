use serde::Deserialize;

#[derive(Deserialize)]
pub struct GithubCallback {
    pub code: String,
}

#[derive(Deserialize)]
pub struct GithubUser {
    pub id: i64,
    pub login: String,
    pub avatar_url: String,
}
