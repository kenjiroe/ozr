use crate::core::policy::ActionKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolDefinition {
    pub name: String,
    pub description: String,
    pub action_kind: ActionKind,
}

/// Classify a tool name when it is not present in the MCP catalog.
/// Returns `None` when no explicit heuristic matches (security-first unknown handling).
pub fn classify_action_kind_from_name(tool_name: &str) -> Option<ActionKind> {
    let lower = tool_name.to_ascii_lowercase();
    if lower.contains("shell")
        || lower.starts_with("run_")
        || lower.starts_with("exec")
        || lower.contains("bash")
        || lower.contains("terminal")
    {
        return Some(ActionKind::Shell);
    }
    if lower.contains("fetch")
        || lower.contains("http")
        || lower.contains("network")
        || lower.starts_with("get_url")
    {
        return Some(ActionKind::Network);
    }
    if lower.contains("write")
        || lower.contains("edit")
        || lower.contains("patch")
        || lower.contains("delete")
        || lower.contains("create")
        || lower.contains("move")
        || lower.contains("rename")
    {
        return Some(ActionKind::Write);
    }
    if lower.starts_with("read_")
        || lower.starts_with("list_")
        || lower.starts_with("search_")
        || lower.starts_with("get_")
        || lower == "directory_tree"
        || lower == "read_file"
    {
        return Some(ActionKind::Read);
    }
    None
}

pub fn infer_action_kind(tool_name: &str) -> ActionKind {
    classify_action_kind_from_name(tool_name).unwrap_or(ActionKind::Shell)
}

pub fn build_tool_definition(name: &str, description: &str) -> McpToolDefinition {
    McpToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        action_kind: infer_action_kind(name),
    }
}

pub fn tool_names(catalog: &[McpToolDefinition]) -> Vec<String> {
    catalog.iter().map(|tool| tool.name.clone()).collect()
}

pub fn action_kind_for_tool(catalog: &[McpToolDefinition], tool: &str) -> ActionKind {
    catalog
        .iter()
        .find(|def| def.name == tool)
        .map(|def| def.action_kind)
        .unwrap_or_else(|| classify_action_kind_from_name(tool).unwrap_or(ActionKind::Shell))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_shell_tools_to_shell_kind() {
        assert_eq!(infer_action_kind("run_shell"), ActionKind::Shell);
    }

    #[test]
    fn maps_read_tools_to_read_kind() {
        assert_eq!(infer_action_kind("read_file"), ActionKind::Read);
        assert_eq!(infer_action_kind("read_text_file"), ActionKind::Read);
        assert_eq!(infer_action_kind("list_directory"), ActionKind::Read);
    }

    #[test]
    fn unknown_name_falls_back_to_shell() {
        assert_eq!(classify_action_kind_from_name("mystery_tool"), None);
        assert_eq!(infer_action_kind("mystery_tool"), ActionKind::Shell);
        assert_eq!(action_kind_for_tool(&[], "mystery_tool"), ActionKind::Shell);
    }

    #[test]
    fn resolves_action_kind_from_catalog() {
        let catalog = vec![
            build_tool_definition("read_text_file", "read"),
            build_tool_definition("write_file", "write"),
        ];
        assert_eq!(
            action_kind_for_tool(&catalog, "write_file"),
            ActionKind::Write
        );
        assert_eq!(
            action_kind_for_tool(&catalog, "unknown_tool"),
            ActionKind::Shell
        );
    }

    #[test]
    fn heuristic_read_outside_catalog_stays_read() {
        assert_eq!(
            action_kind_for_tool(&[], "read_text_file"),
            ActionKind::Read
        );
    }
}
