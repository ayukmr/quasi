use crate::oauth::Credentials;
use crate::render::{self, Renderer};

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use minreq::{Request, ResponseLazy};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const API_URL: &str = "https://api.anthropic.com/v1/messages?beta=true";

#[derive(Clone, Debug, Serialize)]
pub struct Body {
    pub model: String,
    pub max_tokens: u32,
    pub stream: bool,
    pub system: Vec<Content>,
    pub tools: Value,
    pub messages: Vec<Message>,
    pub thinking: Thinking,
    pub output_config: OutputConfig,
}

impl Body {
    pub fn add_cache(mut self) -> Self {
        for msg in self.messages.iter_mut().rev() {
            if matches!(msg.role, Role::User) {
                let Some(last) = msg.content.last_mut() else {
                    continue;
                };

                match last {
                    Content::Text { cache_control, .. } | Content::ToolResult { cache_control, .. } => {
                        *cache_control = Some(CacheControl { kind: "ephemeral".to_string() });
                        break;
                    }
                    _ => {}
                }
            }
        }
        self
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Thinking {
    #[serde(rename = "type")]
    pub kind: String,
    pub display: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutputConfig {
    pub effort: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Content>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    Thinking { thinking: String, signature: String },
    RedactedThinking { data: String },
    ToolUse { id: String, name: String, input: HashMap<String, Value> },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CacheControl {
    #[serde(rename = "type")]
    kind: String,
}

pub fn req(body: &Body, creds: &Credentials, renderer: Arc<Mutex<Renderer>>) -> Result<Response> {
    let json = serde_json::to_string(body)?;

    renderer.lock().unwrap().set_status(Some("contacting"));

    let resp = add_headers(minreq::post(API_URL), creds)
        .with_body(json)
        .send_lazy()?;

    handle_resp(resp, renderer)
}

fn add_headers(req: Request, creds: &Credentials) -> Request {
    req
        .with_header("Authorization", format!("Bearer {}", creds.access))
        .with_header("Anthropic-Version", "2023-06-01")
        .with_header("Anthropic-Beta", "claude-code-20250219,oauth-2025-04-20")
        .with_header("Content-Type", "application/json")
        .with_header("Accept", "text/event-stream")
        .with_header("User-Agent", "claude-cli/2.1.280")
        .with_header("X-App", "cli")
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    ContentBlockStart { content_block: Content },
    ContentBlockDelta { index: u32, delta: ContentDelta },
    MessageDelta {
        delta: MessageDelta,
        usage: Usage,
    },
    ContentBlockStop,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentDelta {
    TextDelta {
        #[serde(rename = "text")]
        delta: String,
    },
    ThinkingDelta {
        #[serde(rename = "thinking")]
        delta: String,
    },
    SignatureDelta {
        #[serde(rename = "signature")]
        delta: String,
    },
    InputJsonDelta {
        #[serde(rename = "partial_json")]
        delta: String,
    },
}

#[derive(Debug, Deserialize)]
struct MessageDelta {
    stop_reason: StopReason,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    #[serde(other)]
    Unknown,
}

#[derive(Debug)]
pub struct Response {
    pub content: Vec<Content>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

fn handle_resp(resp: ResponseLazy, renderer: Arc<Mutex<Renderer>>) -> Result<Response> {
    let mut reader = BufReader::new(resp);
    let mut content = Vec::new();

    let mut line: Vec<u8> = Vec::new();
    let mut cur = None;
    let mut rendered = 0;
    let mut json_buf = String::new();

    let (stop_reason, usage) = loop {
        line.clear();

        if reader.read_until(b'\n', &mut line)? == 0 {
            bail!("stream ended before message end");
        }

        let text = String::from_utf8_lossy(&line);
        let Some(json) = text.strip_prefix("data: ") else {
            continue;
        };

        let event: Event = serde_json::from_str(json)?;

        match event {
            Event::ContentBlockStart { content_block } => {
                let stat = match content_block {
                    Content::Text { .. } => "generating",
                    Content::Thinking { .. } | Content::RedactedThinking { .. } => "pondering",
                    Content::ToolUse { .. } => "drafting",
                    _ => "futzing",
                };
                renderer.lock().unwrap().set_status(Some(stat));

                cur = Some(content_block);

                rendered = 0;
                json_buf.clear();
            }
            Event::ContentBlockDelta { index, delta } => {
                // TODO in theory use index
                match delta {
                    ContentDelta::TextDelta { delta } => {
                        if let Some(Content::Text { text, .. }) = cur.as_mut() {
                            text.push_str(&delta);
                            let pending = &text[rendered..];

                            if !pending.trim().is_empty() {
                                let full = render::format(text);
                                renderer.lock().unwrap().write_pretty(pending, &full);
                                rendered = text.len();
                            }
                        }
                    }
                    ContentDelta::ThinkingDelta { delta } => {
                        if let Some(Content::Thinking { thinking, .. }) = cur.as_mut() {
                            thinking.push_str(&delta);
                            let pending = &thinking[rendered..];

                            if !pending.trim().is_empty() {
                                renderer.lock().unwrap().write_plain(&delta);
                                rendered = thinking.len();
                            }
                        }
                    }
                    ContentDelta::SignatureDelta { delta } => {
                        if let Some(Content::Thinking { signature, .. }) = cur.as_mut() {
                            signature.push_str(&delta);
                        }
                    }
                    ContentDelta::InputJsonDelta { delta } => {
                        json_buf.push_str(&delta);
                    }
                }
            }
            Event::ContentBlockStop => {
                if !json_buf.is_empty() && let Some(Content::ToolUse { input, .. }) = cur.as_mut() {
                    *input = serde_json::from_str(&json_buf)?;
                }

                let textual = matches!(cur, Some(Content::Text { .. })) || matches!(cur, Some(Content::Thinking { .. }));

                content.push(cur.take().unwrap());
                renderer.lock().unwrap().set_status(None);

                if textual {
                    renderer.lock().unwrap().finish();
                }
            }
            Event::MessageDelta { delta: MessageDelta { stop_reason }, usage } => {
                break (stop_reason, usage);
            }
            Event::Unknown => {}
        }
    };

    Ok(Response {
        content,
        stop_reason,
        usage,
    })
}
