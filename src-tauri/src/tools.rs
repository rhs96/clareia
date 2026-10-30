use std::{collections::HashMap, sync::Mutex, time::{Instant,Duration}, os::windows::process::CommandExt};
use serde_json::{Value,json};
use tauri::Emitter;
use tauri_plugin_updater::{UpdaterExt,Update};
#[derive(Clone)]struct Operation{action:String,args:Value,created:Instant}
#[derive(Default)]pub struct Tools {plans:Mutex<HashMap<String,Operation>>, pub update:Mutex<Option<Update>>}
pub fn ps(action:&str,args:&Value)->Result<Value,String>{
 let output=std::process::Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")
 .args(["-NoLogo","-NoProfile","-NonInteractive","-Command","& ([ScriptBlock]::Create($env:CLAREIA_SCRIPT))"])
 .env("CLAREIA_SCRIPT",include_str!("tools.ps1")).env("CLAREIA_ACTION",action).env("CLAREIA_ARGS",args.to_string()).creation_flags(0x08000000).output().map_err(|e|e.to_string())?;
 if !output.status.success(){return Err(String::from_utf8_lossy(&output.stderr).chars().take(2500).collect())}
 serde_json::from_slice(&output.stdout).map_err(|_|"A ferramenta não retornou dados válidos.".into())
}
#[tauri::command]pub async fn tool_query(action:String,args:Value)->Result<Value,String>{
 if !["inventory","scoop_list","scoop_search","scoop_status","residues","quarantine_list","windows_scan"].contains(&action.as_str()){return Err("Consulta não permitida".into())}
 tauri::async_runtime::spawn_blocking(move||ps(&action,&args)).await.map_err(|e|e.to_string())?
}
#[tauri::command]pub async fn prepare_operation(action:String,args:Value,state:tauri::State<'_,Tools>)->Result<Value,String>{
 if !["terminate","scoop_install","scoop_update","scoop_uninstall","scoop_refresh","uninstall","quarantine","restore","windows_install"].contains(&action.as_str()){return Err("Operação não permitida".into())}
 let a=action.clone();let input=args.clone();
 let review=tauri::async_runtime::spawn_blocking(move||{if a=="terminate"{let pid=input["pid"].as_u64().ok_or("PID inválido")? as u32;let ticks=input["startedTicks"].as_str().ok_or("Identidade indisponível")?;validate_process(pid,ticks)?;Ok(json!({"title":"Encerrar processo","description":format!("Encerrar PID {pid}. Trabalho não salvo poderá ser perdido. Apenas este processo será encerrado, sem finalizar sua árvore."),"args":input}))}else{ps(&format!("prepare_{a}"),&input)}}).await.map_err(|e|e.to_string())??;
 let token=uuid::Uuid::new_v4().to_string();let mut plans=state.plans.lock().unwrap();plans.retain(|_,p|p.created.elapsed()<Duration::from_secs(300));
 plans.insert(token.clone(),Operation{action,args:review["args"].clone(),created:Instant::now()});Ok(json!({"token":token,"title":review["title"],"description":review["description"],"details":review["details"]}))
}
#[tauri::command]pub async fn execute_operation(token:String,state:tauri::State<'_,Tools>)->Result<Value,String>{
 let p=state.plans.lock().unwrap().remove(&token).ok_or("A confirmação expirou ou já foi usada.")?;
 if p.created.elapsed()>Duration::from_secs(300){return Err("A confirmação expirou. Prepare novamente.".into())}
 tauri::async_runtime::spawn_blocking(move||{if p.action=="terminate"{terminate(p.args["pid"].as_u64().unwrap() as u32,p.args["startedTicks"].as_str().unwrap())?;Ok(json!({"message":"Processo encerrado."}))}else{ps(&p.action,&p.args)}}).await.map_err(|e|e.to_string())?
}
fn process_handle(pid:u32,ticks:&str,kill:bool)->Result<windows::Win32::Foundation::HANDLE,String>{
 use windows::Win32::{Foundation::{FILETIME,CloseHandle},System::Threading::*};
 if pid<8||pid==std::process::id(){return Err("Processo do sistema ou do próprio Clareia protegido.".into())}
 unsafe{let access=if kill{PROCESS_QUERY_LIMITED_INFORMATION|PROCESS_TERMINATE}else{PROCESS_QUERY_LIMITED_INFORMATION};let h=OpenProcess(access,false,pid).map_err(|_|"Não foi possível acessar o processo. Ele pode estar protegido ou já ter encerrado.")?;
 let result=(||{let(mut c,mut e,mut k,mut u)=(FILETIME::default(),FILETIME::default(),FILETIME::default(),FILETIME::default());GetProcessTimes(h,&mut c,&mut e,&mut k,&mut u).map_err(|e|e.to_string())?;let actual=(((c.dwHighDateTime as u64)<<32)|c.dwLowDateTime as u64).to_string();if ticks!=actual{return Err("A identidade do processo mudou. Atualize a lista.".into())}
 let mut critical=windows::Win32::Foundation::BOOL::default();IsProcessCritical(h,&mut critical).map_err(|e|e.to_string())?;if critical.as_bool(){return Err("O Windows marcou este processo como crítico.".into())}
 let mut b=vec![0u16;32768];let mut size=b.len() as u32;QueryFullProcessImageNameW(h,PROCESS_NAME_WIN32,windows::core::PWSTR(b.as_mut_ptr()),&mut size).map_err(|e|e.to_string())?;
 let path=String::from_utf16_lossy(&b[..size as usize]).to_ascii_lowercase();let root=std::env::var("SystemRoot").unwrap_or("C:\\Windows".into()).to_ascii_lowercase()+"\\";
 if path.starts_with(&root){return Err("Processos da pasta do Windows são protegidos neste painel.".into())}Ok(())})();if let Err(e)=result{let _=CloseHandle(h);return Err(e)}Ok(h)}
}
fn validate_process(pid:u32,ticks:&str)->Result<(),String>{let h=process_handle(pid,ticks,false)?;unsafe{let _=windows::Win32::Foundation::CloseHandle(h);}Ok(())}
fn terminate(pid:u32,ticks:&str)->Result<(),String>{let h=process_handle(pid,ticks,true)?;unsafe{let result=windows::Win32::System::Threading::TerminateProcess(h,1).map_err(|e|e.to_string());let _=windows::Win32::Foundation::CloseHandle(h);result}}
#[tauri::command]pub fn window_action(action:String,window:tauri::WebviewWindow)->Result<(),String>{match action.as_str(){"minimize"=>window.minimize(),"maximize"=>if window.is_maximized().unwrap_or(false){window.unmaximize()}else{window.maximize()},"close"=>window.close(),"drag"=>window.start_dragging(),_=>return Err("Ação inválida".into())}.map_err(|e|e.to_string())}
#[tauri::command]pub async fn check_update(app:tauri::AppHandle,state:tauri::State<'_,Tools>)->Result<Value,String>{
 let update=app.updater().map_err(|e|e.to_string())?.check().await.map_err(|e|format!("Não foi possível consultar o GitHub Releases: {e}"))?;
 let response=match &update{Some(u)=>json!({"available":true,"version":u.version,"notes":u.body}),None=>json!({"available":false,"version":app.package_info().version.to_string()})};*state.update.lock().unwrap()=update;Ok(response)
}
#[tauri::command]pub async fn install_update(app:tauri::AppHandle,state:tauri::State<'_,Tools>)->Result<(),String>{
 let u=state.update.lock().unwrap().take().ok_or("Verifique as atualizações novamente.")?;
 let progress=app.clone();let mut received=0usize;
 u.download_and_install(move|chunk,total|{received+=chunk;let _=progress.emit("update-progress",json!({"received":received,"total":total}));},||{}).await.map_err(|e|e.to_string())?;
 app.restart();
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]#[ignore]fn sleeper(){std::thread::sleep(Duration::from_secs(60));}
 #[test]fn process_identity_and_termination(){
 use windows::Win32::{Foundation::{FILETIME,CloseHandle},System::Threading::*};
 let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--ignored","--exact","tools::tests::sleeper"]).creation_flags(0x08000000).spawn().unwrap();
 std::thread::sleep(Duration::from_millis(250));let pid=child.id();let ticks=unsafe{let h=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid).unwrap();let(mut c,mut e,mut k,mut u)=(FILETIME::default(),FILETIME::default(),FILETIME::default(),FILETIME::default());GetProcessTimes(h,&mut c,&mut e,&mut k,&mut u).unwrap();let _=CloseHandle(h);(((c.dwHighDateTime as u64)<<32)|c.dwLowDateTime as u64).to_string()};
 assert!(terminate(pid,"1").is_err());assert!(child.try_wait().unwrap().is_none());terminate(pid,&ticks).unwrap();assert!(!child.wait().unwrap().success());assert!(validate_process(std::process::id(),&ticks).is_err());
 }
}

