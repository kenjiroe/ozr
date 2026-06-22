mod ozr_process;

use ozr_process::{ConnectionMode, OzrProcess};
use serde::Serialize;
use tauri::{Manager, RunEvent, State};

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct ApiBootInfo {
    base_url: String,
    mode: &'static str,
}

#[tauri::command]
fn prepare_api(state: State<'_, OzrProcess>) -> Result<ApiBootInfo, String> {
    state.start()?;
    let base_url = state.wait_until_healthy(40, 200)?;
    let mode = match state.connection_mode() {
        ConnectionMode::Spawned => "spawned",
        ConnectionMode::External => "external",
    };
    Ok(ApiBootInfo { base_url, mode })
}

#[tauri::command]
fn get_api_base_url(state: State<'_, OzrProcess>) -> String {
    state.api_base().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(OzrProcess::new())
        .invoke_handler(tauri::generate_handler![prepare_api, get_api_base_url])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            if let Some(ozr) = app_handle.try_state::<OzrProcess>() {
                ozr.stop();
            }
        }
    });
}
