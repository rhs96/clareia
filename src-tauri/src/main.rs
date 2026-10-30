#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod network;
mod tools;
use std::{os::windows::process::CommandExt, sync::Arc};

#[tauri::command]
async fn snapshot(collector:tauri::State<'_,Arc<network::Collector>>) -> Result<serde_json::Value, String> {
 let collector=collector.inner().clone();
 tauri::async_runtime::spawn_blocking(move || {
 let output = std::process::Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")
 .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", include_str!("collect.ps1")])
 .creation_flags(0x08000000).output().map_err(|e| e.to_string())?;
 if !output.status.success() { return Err(String::from_utf8_lossy(&output.stderr).to_string()); }
 let mut snapshot:serde_json::Value=serde_json::from_slice(&output.stdout).map_err(|e| format!("Resposta inválida da coleta: {e}"))?;
 snapshot["network"]=collector.snapshot();Ok(snapshot)
 }).await.map_err(|e| e.to_string())?
}
#[tauri::command]
fn set_capture_paused(paused:bool,collector:tauri::State<'_,Arc<network::Collector>>){collector.pause(paused);}
#[tauri::command]
fn restart_elevated(app:tauri::AppHandle)->Result<(),String>{
 use windows::{core::PCWSTR,Win32::{UI::{Shell::ShellExecuteW,WindowsAndMessaging::SW_SHOWNORMAL},Foundation::HWND}};
 let exe=std::env::current_exe().map_err(|e|e.to_string())?;
 let path:Vec<u16>=exe.as_os_str().encode_wide().chain(Some(0)).collect();
 let verb:Vec<u16>="runas".encode_utf16().chain(Some(0)).collect();
 let result=unsafe{ShellExecuteW(HWND::default(),PCWSTR(verb.as_ptr()),PCWSTR(path.as_ptr()),PCWSTR::null(),PCWSTR::null(),SW_SHOWNORMAL)};
 if result.0 as isize<=32{return Err("A permissão não foi concedida. A janela atual continua aberta.".into())}app.exit(0);Ok(())
}
use std::os::windows::ffi::OsStrExt;
fn main() {
 let args:Vec<String>=std::env::args().collect();
 if args.get(1).is_some_and(|s|s=="--self-test") {let code=network::self_test(args.get(2).map(String::as_str).unwrap_or("clareia-teste.json"));std::process::exit(code);}
 let collector=Arc::new(network::Collector::start());let exit_collector=collector.clone();
 let app=tauri::Builder::default().manage(collector).manage(tools::Tools::default()).plugin(tauri_plugin_updater::Builder::new().build())
 .invoke_handler(tauri::generate_handler![snapshot,set_capture_paused,restart_elevated,tools::tool_query,tools::prepare_operation,tools::execute_operation,tools::window_action,tools::check_update,tools::install_update])
 .build(tauri::generate_context!()).expect("Falha ao iniciar Clareia");
 app.run(move|_,event|{if matches!(event,tauri::RunEvent::Exit){exit_collector.stop();}});
}


