use ferrisetw::{parser::Parser, provider::{Provider, kernel_providers::{self, KernelProvider}}, schema_locator::SchemaLocator, trace::{KernelTrace, TraceTrait, TraceProperties}, EventRecord};
use serde::Serialize;
use std::{collections::HashMap, net::IpAddr, sync::{Arc, Mutex, mpsc}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use windows::{core::PCWSTR, Win32::{Foundation::{CloseHandle, FILETIME}, System::{Threading::{OpenProcess, GetProcessTimes, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_NAME_WIN32}, Diagnostics::Etw::{ControlTraceW, CONTROLTRACE_HANDLE, EVENT_TRACE_PROPERTIES, EVENT_TRACE_CONTROL_QUERY}}}};

#[derive(Clone, Serialize, Default, Debug)]
#[serde(rename_all="camelCase")]
pub struct Flow { pub protocol: String, pub local: String, pub local_port:u16, pub remote:String, pub port:u16, pub sent:u64, pub received:u64, pub last_seen:u64 }
#[derive(Clone, Serialize, Default, Debug)]
#[serde(rename_all="camelCase")]
pub struct ProcessTraffic { pub pid:u32, pub started_ticks:String, pub name:String, pub path:String, pub sent:u64, pub received:u64, pub tcp_sent:u64, pub tcp_received:u64, pub udp_sent:u64, pub udp_received:u64, pub flows:HashMap<String,Flow> }
#[derive(Default, Serialize)]
#[serde(rename_all="camelCase")]
pub struct Data { pub running:bool, pub paused:bool, pub error:Option<String>, pub parse_errors:u64, pub events_lost:Option<u32>, pub realtime_buffers_lost:Option<u32>, pub stats_error:Option<String>, pub evicted_flows:u64, pub events:u64, pub omitted_events:u64, #[serde(skip)] pub capture_after_ticks:u64, #[serde(skip)] pub tracked_flows:usize, pub processes:HashMap<String, ProcessTraffic> }
pub struct Collector { pub data:Arc<Mutex<Data>>, stop:Mutex<Option<mpsc::Sender<()>>>, worker:Mutex<Option<std::thread::JoinHandle<()>>> }
#[derive(Clone)]
struct Identity { started:String, name:String, path:String, refreshed:Instant }
fn filetime(v:FILETIME)->u64 { ((v.dwHighDateTime as u64)<<32)|v.dwLowDateTime as u64 }
fn identity(pid:u32)->Identity {
 let mut id=Identity{started:"unknown".into(),name:format!("Processo {pid}"),path:String::new(),refreshed:Instant::now()};
 unsafe { if let Ok(h)=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid) {
 let(mut created,mut exit,mut kernel,mut user)=(FILETIME::default(),FILETIME::default(),FILETIME::default(),FILETIME::default());
 if GetProcessTimes(h,&mut created,&mut exit,&mut kernel,&mut user).is_ok(){ id.started=filetime(created).to_string(); }
 let mut path=vec![0u16;32768];let mut size=path.len() as u32;
 if QueryFullProcessImageNameW(h,PROCESS_NAME_WIN32,windows::core::PWSTR(path.as_mut_ptr()),&mut size).is_ok(){id.path=String::from_utf16_lossy(&path[..size as usize]);id.name=std::path::Path::new(&id.path).file_stem().map(|s|s.to_string_lossy().into_owned()).unwrap_or(id.name);}
 let _=CloseHandle(h);
 }} id
}
fn query_loss(name:&str)->Result<(u32,u32),String>{
 // Vec<u64> provides alignment for EVENT_TRACE_PROPERTIES. Extra space holds logger name.
 let mut buffer=vec![0u64;(std::mem::size_of::<EVENT_TRACE_PROPERTIES>()+2048).div_ceil(8)];
 let p=buffer.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
 let wide:Vec<u16>=name.encode_utf16().chain(Some(0)).collect();
 unsafe{(*p).Wnode.BufferSize=(buffer.len()*8) as u32;(*p).LoggerNameOffset=std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;
 let code=ControlTraceW(CONTROLTRACE_HANDLE{Value:0},PCWSTR(wide.as_ptr()),p,EVENT_TRACE_CONTROL_QUERY);
 if code.0!=0{return Err(format!("Consulta ETW: {}",code.0))}Ok(((*p).EventsLost,(*p).RealTimeBuffersLost))}
}
#[derive(Debug)]
struct Packet {pid:u32, sent:bool, size:u64, protocol:&'static str, local:String, local_port:u16, remote:String, port:u16, event_ticks:u64}
fn parse(record:&EventRecord,locator:&SchemaLocator,protocol:&'static str)->Option<Result<Packet,String>>{
 let sent=match record.opcode(){10|26=>true,11|27=>false,_=>return None};
 Some((||{let schema=locator.event_schema(record).map_err(|e|format!("{e:?}"))?;let p=Parser::create(record,&schema);
 let pid=p.try_parse::<u32>("PID").map_err(|e|format!("PID: {e:?}"))?;
 let size=p.try_parse::<u32>("size").map_err(|e|format!("size: {e:?}"))? as u64;
 let src=p.try_parse::<IpAddr>("saddr").map_err(|e|format!("saddr: {e:?}"))?.to_string();
 let dst=p.try_parse::<IpAddr>("daddr").map_err(|e|format!("daddr: {e:?}"))?.to_string();
 let sport=u16::from_be(p.try_parse::<u16>("sport").map_err(|e|format!("sport: {e:?}"))?);
 let dport=u16::from_be(p.try_parse::<u16>("dport").map_err(|e|format!("dport: {e:?}"))?);
 let ticks=record.raw_timestamp() as u64;
 let(local,local_port,remote,port)=if sent{(src,sport,dst,dport)}else{(dst,dport,src,sport)};
 Ok(Packet{pid,sent,size,protocol,local,local_port,remote,port,event_ticks:ticks})})())
}
fn apply(data:&mut Data,packet:Packet,id:&Identity){
 if data.paused || packet.event_ticks < data.capture_after_ticks{return}
 // Do not attribute a delayed event to a newer process which reused the PID.
 let valid=id.started.parse::<u64>().is_ok_and(|created|created<=packet.event_ticks);
 let started=if valid{id.started.clone()}else{"unknown".into()};let key=format!("{}:{}",packet.pid,started);
 let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
 if !data.processes.contains_key(&key) && data.processes.len() >= 2048 {data.omitted_events+=1;return;}
 let proc=data.processes.entry(key).or_insert_with(||ProcessTraffic{pid:packet.pid,started_ticks:started,name:if valid{id.name.clone()}else{format!("Processo {} (identidade indisponível)",packet.pid)},path:if valid{id.path.clone()}else{String::new()},..Default::default()});
 if packet.sent {proc.sent+=packet.size;if packet.protocol=="TCP"{proc.tcp_sent+=packet.size}else{proc.udp_sent+=packet.size}}
 else{proc.received+=packet.size;if packet.protocol=="TCP"{proc.tcp_received+=packet.size}else{proc.udp_received+=packet.size}}
 let key=format!("{}|{}|{}|{}|{}",packet.protocol,packet.local,packet.local_port,packet.remote,packet.port);
 data.events+=1;
 if !proc.flows.contains_key(&key)&&proc.flows.len()>=64{if let Some(old)=proc.flows.iter().min_by_key(|(_,f)|f.last_seen).map(|(k,_)|k.clone()){proc.flows.remove(&old);data.evicted_flows+=1;data.tracked_flows-=1;}}
 if !proc.flows.contains_key(&key){if data.tracked_flows>=16384{data.evicted_flows+=1;return;}data.tracked_flows+=1;}
 let f=proc.flows.entry(key).or_insert_with(||Flow{protocol:packet.protocol.into(),local:packet.local,local_port:packet.local_port,remote:packet.remote,port:packet.port,..Default::default()});f.last_seen=now;if packet.sent{f.sent+=packet.size}else{f.received+=packet.size}
}
fn callback(shared:Arc<Mutex<Data>>,protocol:&'static str,identities:Arc<Mutex<HashMap<u32,Identity>>>)->impl FnMut(&EventRecord,&SchemaLocator)+Send+Sync+'static{
 
 move |r,l|{if let Some(parsed)=parse(r,l,protocol){match parsed{Ok(packet)=>{
 let mut ids=identities.lock().unwrap();let refresh=ids.get(&packet.pid).is_none_or(|id|id.refreshed.elapsed()>Duration::from_secs(1));
 if refresh{if ids.len()>4096{ids.clear();}ids.insert(packet.pid,identity(packet.pid));}if let Some(id)=ids.get(&packet.pid){if let Ok(mut data)=shared.lock(){apply(&mut data,packet,id)}}
 },Err(_)=>{if let Ok(mut d)=shared.lock(){d.parse_errors+=1;}}}}}
}
impl Collector {
 pub fn start()->Self{
 let shared=Arc::new(Mutex::new(Data::default()));let data=shared.clone();let(tx,rx)=mpsc::channel();
 let worker=std::thread::spawn(move||{
 let udp=KernelProvider::new(windows::core::GUID::from_u128(0xbf3a50c5_a9c9_4988_a005_2df0b7c80f80),0x00010000);
 let ids=Arc::new(Mutex::new(HashMap::<u32,Identity>::new()));let lifecycle_ids=ids.clone();
 let process=Provider::kernel(&kernel_providers::PROCESS_PROVIDER).add_callback(move |r:&EventRecord,l:&SchemaLocator|{if matches!(r.opcode(),1|2|3|4){let pid=l.event_schema(r).ok().and_then(|s|Parser::create(r,&s).try_parse::<u32>("ProcessId").ok());let mut cache=lifecycle_ids.lock().unwrap();if let Some(pid)=pid{cache.remove(&pid);}else{cache.clear();}}}).build();
 let tcp=Provider::kernel(&kernel_providers::TCP_IP_PROVIDER).add_callback(callback(data.clone(),"TCP",ids.clone())).build();
 let udp=Provider::kernel(&udp).add_callback(callback(data.clone(),"UDP",ids.clone())).build();
 let name=format!("Clareia-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis());
 let properties=TraceProperties{buffer_size:64,min_buffer:32,max_buffer:128,..Default::default()};
 match KernelTrace::new().named(name.clone()).set_trace_properties(properties).enable(process).enable(tcp).enable(udp).start(){
 Ok((trace,handle))=>{data.lock().unwrap().running=true;let errors=data.clone();let consumer=std::thread::spawn(move||{let result=KernelTrace::process_from_handle(handle);if let Err(e)=result{let mut d=errors.lock().unwrap();d.running=false;d.error=Some(format!("Consumo ETW interrompido: {e:?}"));}});
 loop{match rx.recv_timeout(Duration::from_secs(1)){Ok(_)|Err(mpsc::RecvTimeoutError::Disconnected)=>break,Err(mpsc::RecvTimeoutError::Timeout)=>{let mut d=data.lock().unwrap();match query_loss(&name){Ok((events,buffers))=>{d.events_lost=Some(events);d.realtime_buffers_lost=Some(buffers);d.stats_error=None;},Err(e)=>d.stats_error=Some(e)}}}}
 let _=trace.stop();let _=consumer.join();data.lock().unwrap().running=false;
 },Err(e)=>{data.lock().unwrap().error=Some(format!("ETW indisponível: {e:?}. Ative a coleta com permissão de administrador."));}}
 });Self{data:shared,stop:Mutex::new(Some(tx)),worker:Mutex::new(Some(worker))}
 }
 pub fn pause(&self,value:bool){let mut d=self.data.lock().unwrap();d.paused=value;if !value{d.capture_after_ticks=(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()/100+116444736000000000) as u64;}}
 pub fn snapshot(&self)->serde_json::Value{{let mut value=serde_json::to_value(&*self.data.lock().unwrap()).unwrap_or_default();value["sampleTime"]=serde_json::json!(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64);value}}
 pub fn stop(&self){if let Some(tx)=self.stop.lock().unwrap().take(){let _=tx.send(());}if let Some(worker)=self.worker.lock().unwrap().take(){let _=worker.join();}}
}
impl Drop for Collector{fn drop(&mut self){self.stop();}}
#[cfg(test)]mod tests{use super::*;fn packet(sent:bool,protocol:&'static str)->Packet{Packet{pid:7,sent,size:100,protocol,local:"::1".into(),local_port:1234,remote:"::1".into(),port:443,event_ticks:200}}
#[test]fn directions_and_protocols(){let mut d=Data::default();let id=Identity{started:"100".into(),name:"test".into(),path:String::new(),refreshed:Instant::now()};apply(&mut d,packet(true,"TCP"),&id);apply(&mut d,packet(false,"UDP"),&id);let p=&d.processes["7:100"];assert_eq!((p.sent,p.received,p.tcp_sent,p.udp_received),(100,100,100,100));assert_eq!(p.flows.len(),2);d.paused=true;apply(&mut d,packet(true,"UDP"),&id);assert_eq!(d.events,2);}
#[test]fn delayed_event_does_not_join_reused_pid(){let mut d=Data::default();let id=Identity{started:"300".into(),name:"new".into(),path:String::new(),refreshed:Instant::now()};apply(&mut d,packet(true,"TCP"),&id);assert!(d.processes.contains_key("7:unknown"));assert!(!d.processes.contains_key("7:300"));}}

pub fn self_test(path:&str)->i32{
 use std::io::{Read,Write};use std::net::{TcpListener,TcpStream,UdpSocket};
 let c=Collector::start();std::thread::sleep(Duration::from_secs(2));let mut checks=Vec::new();let mut expected_tcp=0u64;let mut expected_udp=0u64;
 if c.data.lock().unwrap().running{
 for address in ["127.0.0.1:0","[::1]:0"]{
 let result=(||->std::io::Result<()>{let listener=TcpListener::bind(address)?;let addr=listener.local_addr()?;
 let server=std::thread::spawn(move||->std::io::Result<()>{let(mut s,_)=listener.accept()?;s.set_read_timeout(Some(Duration::from_secs(5)))?;let mut b=vec![0u8;262144];s.read_exact(&mut b)?;s.write_all(&b)?;Ok(())});
 let mut client=TcpStream::connect_timeout(&addr,Duration::from_secs(5))?;client.set_read_timeout(Some(Duration::from_secs(5)))?;let b=vec![42u8;262144];client.write_all(&b)?;let mut response=vec![0u8;b.len()];client.read_exact(&mut response)?;server.join().unwrap()?;assert_eq!(b,response);Ok(())})();
 checks.push(format!("TCP {address}: {result:?}"));if result.is_ok(){expected_tcp+=524288;}
 let result=(||->std::io::Result<()>{let server=UdpSocket::bind(address)?;server.set_read_timeout(Some(Duration::from_secs(5)))?;let client=UdpSocket::bind(address)?;client.set_read_timeout(Some(Duration::from_secs(5)))?;client.send_to(&vec![1u8;4096],server.local_addr()?)?;let mut b=vec![0u8;8192];let(n,peer)=server.recv_from(&mut b)?;server.send_to(&b[..n],peer)?;let(n,_)=client.recv_from(&mut b)?;assert_eq!(n,4096);Ok(())})();
 checks.push(format!("UDP {address}: {result:?}"));if result.is_ok(){expected_udp+=8192;}
 }
 std::thread::sleep(Duration::from_secs(4));}
 let snapshot=c.snapshot();c.stop();let data=&snapshot["processes"];let pid=std::process::id();let mut tcp_sent=0u64;let mut tcp_recv=0u64;let mut udp_sent=0u64;let mut udp_recv=0u64;
 if let Some(procs)=data.as_object(){for p in procs.values().filter(|p|p["pid"]==pid){tcp_sent+=p["tcpSent"].as_u64().unwrap_or(0);tcp_recv+=p["tcpReceived"].as_u64().unwrap_or(0);udp_sent+=p["udpSent"].as_u64().unwrap_or(0);udp_recv+=p["udpReceived"].as_u64().unwrap_or(0);}}
 let passed=expected_tcp>0&&expected_udp>0&&tcp_sent==expected_tcp&&tcp_recv==expected_tcp&&udp_sent==expected_udp&&udp_recv==expected_udp&&checks.iter().all(|c|c.ends_with("Ok(())"))&&snapshot["parseErrors"]==0&&snapshot["eventsLost"]==0&&snapshot["realtimeBuffersLost"]==0&&snapshot["omittedEvents"]==0;
 let result=serde_json::json!({"passed":passed,"checks":checks,"expectedTcpEachDirection":expected_tcp,"expectedUdpEachDirection":expected_udp,"observed":{"tcpSent":tcp_sent,"tcpReceived":tcp_recv,"udpSent":udp_sent,"udpReceived":udp_recv},"snapshot":snapshot});
 if std::fs::write(path,serde_json::to_vec_pretty(&result).unwrap()).is_err(){return 2;}if passed{0}else{1}
}



