use crate::{render, utils};
use crate::oauth::Credentials;
use crate::render::Renderer;
use crate::sse::{self, Body, Content, Message, OutputConfig, Role, StopReason, Thinking, Usage};
use crate::tools::{self, Tool};

use std::fs;
use std::fmt::Write;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const SYSTEM: &str = "You are quasi (`qsi`), an expert coding assistant. You help users with coding tasks by reading files, executing commands, editing code, and writing new files.

Available tools:
- read: Read file contents
- bash: Execute bash commands
- edit: Make surgical edits to files
- write: Create or overwrite files

Guidelines:
- Use read to examine files before editing
- Use edit for precise changes (old text must match exactly)
- Use write only for new files or complete rewrites
- Use bash for file operations like ls, rg, fd
- Do not use bash when another tool would work
- Bash is sandboxed for file writing, do not try to get around it or use python for writing
- Always use fully qualified paths, ~ will not be expanded to $HOME
- Do not be too eager with writing files - ask the user if it is implied but not said
- When summarizing your actions, output plain text directly - do NOT use cat or bash to display what you did
- Be concise in your responses, do not state multiple solutions unless asked
- Never include emojis in your responses, unless explicitly asked
- Show file paths clearly when working with files";

pub struct Agent {
    pub model: String,
    pub thinking: String,
    renderer: Arc<Mutex<Renderer>>,
    creds: Credentials,
    tools: Vec<Tool>,
    system: Vec<Content>,
    messages: Vec<Message>,
    pub usage: Usage,
    pub yolo: bool,
}

#[derive(Deserialize, Serialize)]
pub struct AgentSave {
    model: String,
    system: Vec<Content>,
    messages: Vec<Message>,
    usage: Usage,
}

impl Agent {
    pub fn new(renderer: Arc<Mutex<Renderer>>) -> Result<Self> {
        Ok(Self {
            model: "sonnet-5-5".to_string(),
            thinking: "medium".to_string(),
            renderer: renderer.clone(),
            creds: Credentials::get(renderer)?,
            tools: tools::tools()?,
            system: vec![
                Content::Text { text: "You are Claude Code, Anthropic's official CLI for Claude.".to_string(), cache_control: None },
                Content::Text { text: SYSTEM.to_string(), cache_control: None },
            ],
            messages: Vec::new(),
            usage: Usage { input_tokens: 0, cache_creation_input_tokens: 0, output_tokens: 0 },
            yolo: false,
        })
    }

    pub fn load(renderer: Arc<Mutex<Renderer>>, name: &str) -> Result<Self> {
        let file = utils::convo_dir()?.join(format!("{name}.json"));
        let json = fs::read_to_string(file)?;
        let save: AgentSave = serde_json::from_str(&json)?;

        let tools = tools::tools()?;
        let mut out = String::new();

        for msg in &save.messages {
            for cont in &msg.content {
                match cont {
                    Content::Text { text, .. } => {
                        match msg.role {
                            Role::User => {
                                writeln!(out, "\n\x1b[35m>\x1b[0m {text}").unwrap();
                            }
                            Role::Assistant => {
                                writeln!(out, "{}", render::format(text)).unwrap();
                            }
                            _ => {}
                        }
                    }
                    Content::Thinking { thinking, .. } if !thinking.trim().is_empty() => {
                        writeln!(out, "{}", render::dimmed(thinking.trim())).unwrap();
                    }
                    Content::ToolUse { name, input, .. } => {
                        let tool = tools.iter().find(|t| t.name == *name).context("invalid tool call")?;
                        writeln!(out, "{}", render::tool(tool, input)).unwrap();
                    }
                    _ => {}
                }
            }
        }

        print!("{out}");

        Ok(Self {
            tools,
            model: save.model,
            thinking: "medium".to_string(),
            renderer: renderer.clone(),
            creds: Credentials::get(renderer)?,
            system: save.system,
            messages: save.messages,
            usage: save.usage,
            yolo: false,
        })
    }

    pub fn save(&self, name: &str) -> Result<()> {
        let save = AgentSave {
            model: self.model.clone(),
            system: self.system.clone(),
            messages: self.messages.clone(),
            usage: self.usage,
        };

        let file = format!("{name}.json");
        let json = serde_json::to_string_pretty(&save)?;

        let dir = utils::convo_dir()?;
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(file), json)?;

        Ok(())
    }

    pub fn reload(&mut self) -> Result<()> {
        self.tools = tools::tools()?;
        Ok(())
    }

    fn body(&self) -> Body {
        Body {
            model: format!("claude-{}", self.model),
            max_tokens: 65_536,
            stream: true,
            system: self.system.clone(),
            tools: self.tools.iter().map(Tool::to_json).collect(),
            messages: self.messages.clone(),
            thinking: Thinking {
                kind: "adaptive".to_string(),
                display: "summarized".to_string(),
            },
            output_config: OutputConfig { effort: self.thinking.clone() },
        }
    }

    pub fn prompt(&mut self, query: &str) -> Result<()> {
        self.messages.push(
            Message {
                role: Role::User,
                content: vec![Content::Text { text: query.to_string(), cache_control: None }],
            }
        );

        let mut resp;

        loop {
            resp = sse::req(&self.body().add_cache(), &self.creds, self.renderer.clone())?;

            self.usage.input_tokens += resp.usage.input_tokens;
            self.usage.cache_creation_input_tokens += resp.usage.cache_creation_input_tokens;
            self.usage.output_tokens += resp.usage.output_tokens;

            self.messages.push(Message { role: Role::Assistant, content: resp.content });
            let content = &self.messages.last().unwrap().content;

            if !matches!(resp.stop_reason, StopReason::ToolUse) {
                break;
            }

            self.renderer.lock().unwrap().set_status(Some("calling tools"));

            let mut tool_res = vec![];

            for c in content {
                if let Content::ToolUse { id, name, input } = c {
                    let tool = self.tools.iter().find(|t| t.name == *name).context("invalid tool call")?;

                    self.renderer.lock().unwrap().write_line(&render::tool(tool, input));

                    let (content, is_error) =
                        if !tool.dangerous || self.yolo || self.renderer.lock().unwrap().confirm() {
                            let resp = (tool.call)(input.clone());
                            let is_err = resp.is_err();

                            (resp.unwrap_or_else(|e| format!("error: {}", e)), is_err)
                        } else {
                            ("error: tool denied by user".to_string(), true)
                        };

                    tool_res.push(
                        Content::ToolResult {
                            content,
                            is_error,
                            tool_use_id: id.to_string(),
                            cache_control: None,
                        },
                    );
                }
            }

            self.renderer.lock().unwrap().set_status(None);
            self.messages.push(Message { role: Role::User, content: tool_res });
        }

        Ok(())
    }
}
