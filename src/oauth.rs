use crate::render::Renderer;

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";

const BIND_ADDR: &str = "127.0.0.1:33541";
const REDIRECT_HOST: &str = "http://localhost:33541";
const REDIRECT_PATH: &str = "/callback";

#[derive(Debug, Deserialize, Serialize)]
pub struct Credentials {
    pub access: String,
    refresh: String,
    expires: SystemTime,
}

#[derive(Deserialize)]
struct AnthropicCredentials {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

struct PKCE {
    verifier: String,
    challenge: String,
    state: String,
}

impl PKCE {
    fn generate() -> Self {
        let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());

        let hash = Sha256::digest(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hash);

        let state = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());

        Self { verifier, challenge, state }
    }
}

fn config_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("$HOME not set")?;
    Ok(PathBuf::from(home).join(".config/quasi"))
}

impl Credentials {
    pub fn get(renderer: Arc<Mutex<Renderer>>) -> Result<Self> {
        let mut creds = Self::from_save().or_else(|_| Self::from_login(renderer.clone()))?;

        if creds.expires < SystemTime::now() {
            creds.refresh(renderer)?;
            let _ = creds.save();
        }

        Ok(creds)
    }

    fn from_save() -> Result<Self> {
        let file = config_dir()?.join("creds.json");
        let json = fs::read_to_string(file)?;
        let creds = serde_json::from_str(&json)?;

        Ok(creds)
    }

    fn save(&self) -> Result<()> {
        let dir = config_dir()?;
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("creds.json"), serde_json::to_string(self)?)?;

        Ok(())
    }

    fn from_login(renderer: Arc<Mutex<Renderer>>) -> Result<Self> {
        let redirect_url = format!("{REDIRECT_HOST}{REDIRECT_PATH}");
        let pkce = PKCE::generate();

        let params = [
            ("code", "true"),
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", &redirect_url),
            ("scope", SCOPES),
            ("code_challenge", &pkce.challenge),
            ("code_challenge_method", "S256"),
            ("state", &pkce.state)
        ];

        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let server = tiny_http::Server::http(BIND_ADDR).unwrap();

        let auth_url = format!("{AUTHORIZE_URL}?{query}");

        renderer.lock().unwrap().write_line(&format!("opening browser, or go to {auth_url}"));
        renderer.lock().unwrap().set_status(Some("waiting on auth"));

        let _ = Command::new("open").arg(&auth_url).status();

        let code = loop {
            let req = server.recv_timeout(Duration::from_secs(120))?.context("timed out waiting for callback")?;

            if req.url().starts_with(REDIRECT_PATH) {
                let url = req.url().to_string();
                let query = url.split_once('?').unwrap().1;

                let mut code = None;
                let mut state = None;

                for pair in query.split('&') {
                    let (k, v) = pair.split_once('=').unwrap();

                    match k {
                        "code" => code = Some(v.to_string()),
                        "state" => state = Some(v.to_string()),
                        _ => {}
                    }
                }

                if code.is_none() || state != Some(pkce.state.clone()) {
                    bail!("query params mismatch");
                }
                req.respond(tiny_http::Response::from_string("registered with quasi ✓"))?;

                break code;
            }

            let _ = req.respond(tiny_http::Response::empty(404));
        };

        let creds = Self::from_auth_code(&code.unwrap(), &pkce)?;
        let _ = creds.save();

        renderer.lock().unwrap().set_status(None);
        renderer.lock().unwrap().write_line("registered token \x1b[32m✓\x1b[0m");

        Ok(creds)
    }

    fn refresh(&mut self, renderer: Arc<Mutex<Renderer>>) -> Result<()> {
        renderer.lock().unwrap().set_status(Some("refreshing token"));

        let body = json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": &self.refresh,
        }).to_string();

        let resp = minreq::post(TOKEN_URL)
            .with_header("Content-Type", "application/json")
            .with_header("Accept", "application/json")
            .with_body(body)
            .send()?;

        let json = resp.as_str()?;
        *self = Self::from_api(json)?;

        renderer.lock().unwrap().set_status(None);
        renderer.lock().unwrap().write_line("refreshed token \x1b[32m✓\x1b[0m");

        Ok(())
    }

    fn from_auth_code(code: &str, pkce: &PKCE) -> Result<Self> {
        let redirect_url = format!("{REDIRECT_HOST}{REDIRECT_PATH}");

        let body = json!({
            "grant_type": "authorization_code",
            "client_id": CLIENT_ID,
            "code": &code,
            "state": &pkce.state,
            "redirect_uri": &redirect_url,
            "code_verifier": &pkce.verifier,
        }).to_string();

        let resp = minreq::post(TOKEN_URL)
            .with_header("Content-Type", "application/json")
            .with_header("Accept", "application/json")
            .with_body(body)
            .send()?;

        let json = resp.as_str()?;
        Self::from_api(json)
    }

    fn from_api(json: &str) -> Result<Self> {
        let creds: AnthropicCredentials = serde_json::from_str(json)?;

        Ok(Self {
            access: creds.access_token,
            refresh: creds.refresh_token,
            expires: SystemTime::now() + Duration::from_secs(creds.expires_in),
        })
    }
}
