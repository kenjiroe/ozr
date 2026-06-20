use std::process::Command;

pub trait WorkflowOrchestrator {
    fn dispatch(&self, request: &str) -> Result<String, String>;
}

#[derive(Debug, Clone)]
pub struct SpecKittyWorkflow {
    command: String,
}

impl SpecKittyWorkflow {
    pub fn new(command: String) -> Self {
        Self { command }
    }
}

impl WorkflowOrchestrator for SpecKittyWorkflow {
    fn dispatch(&self, request: &str) -> Result<String, String> {
        let output = Command::new(&self.command)
            .arg("dispatch")
            .arg(request)
            .output()
            .map_err(|e| e.to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct NoopWorkflow;

impl WorkflowOrchestrator for NoopWorkflow {
    fn dispatch(&self, request: &str) -> Result<String, String> {
        Ok(format!("workflow_disabled: {}", request))
    }
}
