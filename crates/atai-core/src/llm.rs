use std::{env, time::Duration};

use anyhow::{Context, Result, bail};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::responses::{CreateResponse, CreateResponseArgs, Reasoning, ReasoningEffort, Response},
};
use serde::{Deserialize, Serialize};

use crate::config::{ApiKey, ModelConfig};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerationResult {
    pub command: String,
    pub summary: String,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub risk_hints: Vec<String>,
}

impl GenerationResult {
    pub fn validate(&self) -> Result<()> {
        if self.command.trim().is_empty() {
            bail!("The model returned an empty command");
        }
        if self.command.contains('\n') {
            bail!("The model returned a multi-line command, which is not allowed");
        }
        if self.summary.trim().is_empty() {
            bail!("The model did not provide a summary");
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct LlmClient {
    client: Client<OpenAIConfig>,
    http_client: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
    system_prompt: String,
    enable_thinking: Option<bool>,
}

impl LlmClient {
    pub fn new(config: &ModelConfig, system_prompt: String) -> Result<Self> {
        let api_key = ApiKey::resolve(&config.api_key)?;
        let client_config = OpenAIConfig::new()
            .with_api_key(api_key.clone())
            .with_api_base(config.endpoint.clone());

        let http_client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .user_agent("atai/0.1.0")
            .build()
            .context("Failed to build the HTTP client")?;

        let client = Client::with_config(client_config).with_http_client(http_client.clone());
        Ok(Self {
            client,
            http_client,
            endpoint: config.endpoint.clone(),
            api_key,
            model: config.model.clone(),
            system_prompt: format!("{system_prompt}\n\n{}", built_in_system_prompt_suffix()),
            enable_thinking: config.enable_thinking,
        })
    }

    pub async fn generate_command(
        &self,
        goal: &str,
        feedback: &[String],
    ) -> Result<GenerationResult> {
        let request = self.build_request(goal, feedback)?;
        let response = self.call_model(request).await?;

        let content = response
            .output_text()
            .context("The model did not return parseable text output")?;
        let json_text = Self::extract_json(&content)?;
        let result: GenerationResult =
            serde_json::from_str(json_text).context("Failed to parse the model JSON output")?;
        result.validate()?;
        Ok(result)
    }

    fn build_request(&self, goal: &str, feedback: &[String]) -> Result<CreateResponse> {
        let mut request = CreateResponseArgs::default()
            .model(self.model.clone())
            .instructions(self.system_prompt.clone())
            .input(self.build_input(goal, feedback))
            .max_output_tokens(400u32)
            .build()
            .context("Failed to build the Responses API request")?;

        if self.enable_thinking == Some(false) && !self.uses_dashscope_compatible_thinking_flag() {
            request.reasoning = Some(Reasoning {
                effort: Some(ReasoningEffort::None),
                summary: None,
            });
        }

        Ok(request)
    }

    async fn call_model(&self, request: CreateResponse) -> Result<Response> {
        if self.uses_dashscope_compatible_thinking_flag() && self.enable_thinking.is_some() {
            return self.call_model_with_raw_request(&request).await;
        }

        self.client
            .responses()
            .create(request)
            .await
            .context("Failed to call the model")
    }

    async fn call_model_with_raw_request(&self, request: &CreateResponse) -> Result<Response> {
        let body = self.build_raw_request_body(request)?;
        let response = self
            .http_client
            .post(format!("{}/responses", self.endpoint.trim_end_matches('/')))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("Failed to call the model")?;

        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .context("Failed to read the model response body")?;

        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes);
            bail!(
                "Failed to call the model: HTTP {} {}",
                status.as_u16(),
                body.trim()
            );
        }

        serde_json::from_slice(&bytes).context("Failed to parse the model response")
    }

    fn build_raw_request_body(&self, request: &CreateResponse) -> Result<serde_json::Value> {
        let mut body = serde_json::to_value(request)
            .context("Failed to serialize the Responses API request")?;

        if self.uses_dashscope_compatible_thinking_flag()
            && let Some(enable_thinking) = self.enable_thinking
        {
            let object = body
                .as_object_mut()
                .context("The serialized request body is not a JSON object")?;
            object.insert(
                "enable_thinking".to_string(),
                serde_json::Value::Bool(enable_thinking),
            );
        }

        Ok(body)
    }

    fn uses_dashscope_compatible_thinking_flag(&self) -> bool {
        self.endpoint.contains("dashscope")
            || self.endpoint.contains("aliyuncs.com/compatible-mode/")
    }

    fn build_input(&self, goal: &str, feedback: &[String]) -> String {
        let cwd = env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());
        let platform = platform_label();
        let shell = env::var("SHELL").unwrap_or_else(|_| "<unknown>".to_string());

        if feedback.is_empty() {
            format!(
                "Current directory: {cwd}\nCurrent platform: {platform}\nCurrent shell: {shell}\nUser goal: {goal}\nGenerate the first candidate command."
            )
        } else {
            let feedback_text = feedback
                .iter()
                .enumerate()
                .map(|(index, item)| format!("{}. {}", index + 1, item))
                .collect::<Vec<_>>()
                .join("\n");

            format!(
                "Current directory: {cwd}\nCurrent platform: {platform}\nCurrent shell: {shell}\nUser goal: {goal}\nUser feedback:\n{feedback_text}\nRegenerate the command based on the feedback."
            )
        }
    }

    fn extract_json(content: &str) -> Result<&str> {
        let trimmed = content.trim();
        let trimmed = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .unwrap_or(trimmed)
            .trim();
        let trimmed = trimmed.strip_suffix("```").unwrap_or(trimmed).trim();

        let start = trimmed
            .find('{')
            .context("The model output does not contain a JSON object start delimiter")?;
        let end = trimmed
            .rfind('}')
            .context("The model output does not contain a JSON object end delimiter")?;

        if end <= start {
            bail!("The JSON object in the model output is incomplete");
        }

        Ok(&trimmed[start..=end])
    }
}

fn built_in_system_prompt_suffix() -> &'static str {
    r#"Additional runtime rules:
11. Commands must match the current platform's default tool behavior. Do not assume GNU-only flags or short-option grouping when an option takes a separate value.
12. Prefer human-readable output when the user asks for sizes, disk usage, or similar metrics. Use flags such as -h unless the user explicitly asks for raw bytes or machine-readable output.
13. Prefer the simplest correct command that a human can read and verify quickly."#
}

fn platform_label() -> &'static str {
    match env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use async_openai::{
        Client,
        config::OpenAIConfig,
        types::responses::{CreateResponseArgs, ReasoningEffort},
    };

    use super::{GenerationResult, LlmClient};

    fn skip_if_ci_for_local_only_test() -> bool {
        std::env::var_os("CI").is_some()
    }

    fn test_client() -> LlmClient {
        LlmClient {
            client: Client::with_config(OpenAIConfig::new()),
            http_client: reqwest::Client::new(),
            endpoint: "https://api.openai.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.4".to_string(),
            system_prompt: "system".to_string(),
            enable_thinking: Some(false),
        }
    }

    #[test]
    fn rejects_multiline_command() {
        let result = GenerationResult {
            command: "echo first\necho second".to_string(),
            summary: "test".to_string(),
            assumptions: Vec::new(),
            risk_hints: Vec::new(),
        };

        assert!(result.validate().is_err());
    }

    #[test]
    fn extracts_json_from_fenced_block() {
        let raw = r#"
```json
{"command":"ls -la","summary":"list files","assumptions":[],"risk_hints":[]}
```
"#;

        let extracted = LlmClient::extract_json(raw).expect("should extract json");
        assert!(extracted.starts_with('{'));
        assert!(extracted.ends_with('}'));
        assert!(extracted.contains("\"command\":\"ls -la\""));
    }

    #[test]
    fn rejects_text_without_json() {
        let result = LlmClient::extract_json("no json here");
        assert!(result.is_err());
    }

    #[test]
    fn uses_openai_reasoning_none_when_thinking_is_disabled() {
        let client = test_client();

        let request = client
            .build_request("List files", &[])
            .expect("request should build");

        assert_eq!(
            request.reasoning.and_then(|reasoning| reasoning.effort),
            Some(ReasoningEffort::None)
        );
    }

    #[test]
    fn build_input_includes_platform_context() {
        let client = test_client();
        let input = client.build_input(
            "Show the size of each directory in the current project",
            &[],
        );

        assert!(input.contains("Current platform: "));
        assert!(input.contains("Current shell: "));
    }

    #[test]
    fn injects_dashscope_enable_thinking_flag_into_raw_body() {
        // This test is only useful for local compatible-gateway validation and is skipped in CI.
        if skip_if_ci_for_local_only_test() {
            eprintln!("skip injects_dashscope_enable_thinking_flag_into_raw_body in CI");
            return;
        }

        let client = LlmClient {
            client: Client::with_config(OpenAIConfig::new()),
            http_client: reqwest::Client::new(),
            endpoint: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "qwen-plus".to_string(),
            system_prompt: "system".to_string(),
            enable_thinking: Some(false),
        };

        let request = CreateResponseArgs::default()
            .model("qwen-plus")
            .instructions("system")
            .input("Who are you?")
            .build()
            .expect("request should build");

        let body = client
            .build_raw_request_body(&request)
            .expect("body should build");

        assert_eq!(body["enable_thinking"], serde_json::Value::Bool(false));
        assert!(body.get("reasoning").is_none());
    }
}
