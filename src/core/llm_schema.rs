#[derive(Debug, Clone)]
pub struct UnifiedChatRequest {
    pub model: String,
    pub user_prompt: String,
}

#[derive(Debug, Clone)]
pub struct UnifiedChatResponse {
    pub content: String,
}

impl UnifiedChatRequest {
    pub fn new(model: &str, user_prompt: &str) -> Self {
        Self {
            model: model.to_string(),
            user_prompt: user_prompt.to_string(),
        }
    }
}
