use crate::storage;

#[tauri::command]
pub async fn get_usage_summary(
    state: tauri::State<'_, storage::UsageStore>,
) -> Result<storage::UsageSummary, String> {
    state.summary().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_usage(state: tauri::State<'_, storage::UsageStore>) -> Result<(), String> {
    state.clear().map_err(|e| e.to_string())
}
