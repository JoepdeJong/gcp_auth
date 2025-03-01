use std::fs;
use std::sync::RwLock;

use async_trait::async_trait;
use hyper::body::Body;
use hyper::{Method, Request};
use serde::{Deserialize, Serialize};

use crate::authentication_manager::ServiceAccount;
use crate::error::Error;
use crate::types::{HyperClient, Token};
use crate::util::HyperExt;

#[derive(Debug)]
pub(crate) struct DefaultAuthorizedUser {
    token: RwLock<Token>,
    credentials: UserCredentials,
}

impl DefaultAuthorizedUser {
    const DEFAULT_TOKEN_GCP_URI: &'static str = "https://accounts.google.com/o/oauth2/token";
    const USER_CREDENTIALS_PATH: &'static str =
        ".config/gcloud/application_default_credentials.json";

    pub(crate) async fn new(client: &HyperClient) -> Result<Self, Error> {
        tracing::debug!("Loading user credentials file");
        let mut home = dirs_next::home_dir().ok_or(Error::NoHomeDir)?;
        home.push(Self::USER_CREDENTIALS_PATH);

        let file = fs::File::open(home).map_err(Error::UserProfilePath)?;
        let credentials = serde_json::from_reader::<_, UserCredentials>(file)
            .map_err(Error::UserProfileFormat)?;

        Ok(Self {
            token: RwLock::new(Self::get_token(&credentials, client).await?),
            credentials,
        })
    }

    fn build_token_request<T: serde::Serialize>(json: &T) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri(Self::DEFAULT_TOKEN_GCP_URI)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(json).unwrap()))
            .unwrap()
    }

    #[tracing::instrument]
    async fn get_token(cred: &UserCredentials, client: &HyperClient) -> Result<Token, Error> {
        let req = Self::build_token_request(&RefreshRequest {
            client_id: &cred.client_id,
            client_secret: &cred.client_secret,
            grant_type: "refresh_token",
            refresh_token: &cred.refresh_token,
        });

        let token = client
            .request(req)
            .await
            .map_err(Error::OAuthConnectionError)?
            .deserialize()
            .await?;
        Ok(token)
    }
}

#[async_trait]
impl ServiceAccount for DefaultAuthorizedUser {
    async fn project_id(&self, _: &HyperClient) -> Result<String, Error> {
        self.credentials
            .quota_project_id
            .clone()
            .ok_or(Error::NoProjectId)
    }

    fn get_token(&self, _scopes: &[&str]) -> Option<Token> {
        Some(self.token.read().unwrap().clone())
    }

    fn get_token_with_subject(&self, _scopes: &[&str], _subject: Option<&str>) -> Option<Token> {
        Some(self.token.read().unwrap().clone())
    }

    async fn refresh_token(&self, client: &HyperClient, _scopes: &[&str], _subject: Option<&str>) -> Result<Token, Error> {
        let token = Self::get_token(&self.credentials, client).await?;
        *self.token.write().unwrap() = token.clone();
        Ok(token)
    }
}

#[derive(Serialize, Debug)]
struct RefreshRequest<'a> {
    client_id: &'a str,
    client_secret: &'a str,
    grant_type: &'a str,
    refresh_token: &'a str,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct UserCredentials {
    /// Client id
    pub(crate) client_id: String,
    /// Client secret
    pub(crate) client_secret: String,
    /// Project ID
    pub(crate) quota_project_id: Option<String>,
    /// Refresh Token
    pub(crate) refresh_token: String,
    /// Type
    pub(crate) r#type: String,
}
