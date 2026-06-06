use litert_lm_sys as sys;
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;
use anyhow::{Result, anyhow};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use tower_http::cors::CorsLayer;
use tokio::sync::mpsc;

// --- Custom FFI Redefinitions for LiteRT-LM C API ABI Parity ---
pub enum LiteRtLmConversationOptionalArgs {}

extern "C" {
    pub fn litert_lm_conversation_optional_args_create() -> *mut LiteRtLmConversationOptionalArgs;
    pub fn litert_lm_conversation_optional_args_delete(
        optional_args: *mut LiteRtLmConversationOptionalArgs,
    );
    pub fn litert_lm_conversation_optional_args_set_visual_token_budget(
        optional_args: *mut LiteRtLmConversationOptionalArgs,
        visual_token_budget: std::os::raw::c_int,
    );
    pub fn litert_lm_conversation_optional_args_set_max_output_tokens(
        optional_args: *mut LiteRtLmConversationOptionalArgs,
        max_output_tokens: std::os::raw::c_int,
    );
    pub fn litert_lm_conversation_send_message_stream(
        conversation: *mut sys::LiteRtLmConversation,
        message_json: *const std::os::raw::c_char,
        extra_context: *const std::os::raw::c_char,
        optional_args: *const LiteRtLmConversationOptionalArgs,
        callback: sys::LiteRtLmStreamCallback,
        callback_data: *mut std::os::raw::c_void,
    ) -> std::os::raw::c_int;
}

// --- 설정 및 상수 ---
const SOUL_FILE: &str = "soul.txt";
const TOOLS_FILE: &str = "tools.txt";

const DEFAULT_SOUL: &str = "당신의 이름은 AI입니다.\n한국어만 사용하며, 친절하고 명확하게 답변합니다.\n수학적 그래프 시각화가 필요할 경우 반드시 ```latex 수식 ``` 블록을 사용하세요.";

// --- Safe Engine Wrapper ---
pub struct EngineWrapper {
    ptr: ptr::NonNull<sys::LiteRtLmEngine>,
}

unsafe impl Send for EngineWrapper {}
unsafe impl Sync for EngineWrapper {}

impl Drop for EngineWrapper {
    fn drop(&mut self) {
        unsafe {
            sys::litert_lm_engine_delete(self.ptr.as_ptr());
        }
    }
}

impl EngineWrapper {
    pub fn new(model_path: &str, use_gpu: bool) -> Result<Self> {
        let model_path_cstr = CString::new(model_path)?;
        let backend = if use_gpu { "gpu" } else { "cpu" };
        let backend_cstr = CString::new(backend)?;
        let cpu_cstr = CString::new("cpu")?;
        
        let settings = unsafe {
            sys::litert_lm_engine_settings_create(
                model_path_cstr.as_ptr(),
                backend_cstr.as_ptr(),
                cpu_cstr.as_ptr(),
                cpu_cstr.as_ptr(),
            )
        };
        if settings.is_null() {
            return Err(anyhow!("Failed to create engine settings"));
        }
        
        let engine_ptr = unsafe { sys::litert_lm_engine_create(settings) };
        unsafe { sys::litert_lm_engine_settings_delete(settings) };
        
        let ptr = ptr::NonNull::new(engine_ptr).ok_or_else(|| anyhow!("Failed to create engine"))?;
        Ok(Self { ptr })
    }
}

// --- App State ---
#[derive(Clone)]
struct AppState {
    engine: Arc<EngineWrapper>,
    system_prompt: String,
    model_name: String,
}

// --- Request types ---
#[derive(serde::Deserialize, Debug)]
struct ChatRequest {
    messages: Option<Vec<serde_json::Value>>,
    stream: Option<bool>,
    options: Option<serde_json::Value>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    top_k: Option<i64>,
    max_tokens: Option<i64>,
    max_output_tokens: Option<i64>,
    num_predict: Option<i64>,
}

// --- ISO 8601 Utility ---
fn get_iso8601_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

// --- System Prompt Loader ---
fn load_system_prompt() -> String {
    let soul = std::fs::read_to_string(SOUL_FILE).unwrap_or_else(|_| DEFAULT_SOUL.to_string());
    let soul = if soul.trim().is_empty() { DEFAULT_SOUL.to_string() } else { soul };
    
    let tools = std::fs::read_to_string(TOOLS_FILE).unwrap_or_default();
    if tools.trim().is_empty() {
        soul
    } else {
        format!("{}\n\n{}", soul, tools)
    }
}

// --- Tools Loader ---
fn load_merged_tools() -> String {
    let static_tools: serde_json::Value = if let Ok(content) = std::fs::read_to_string("tools.json") {
        serde_json::from_str(&content).unwrap_or(serde_json::json!([]))
    } else {
        serde_json::json!([])
    };
    let mut static_tools_arr = if let Some(arr) = static_tools.as_array() {
        arr.clone()
    } else {
        vec![]
    };
    
    let dynamic_registry: serde_json::Value = if let Ok(content) = std::fs::read_to_string("dynamic_tools/registry.json") {
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    
    if let Some(obj) = dynamic_registry.as_object() {
        for (_, spec) in obj {
            static_tools_arr.push(spec.clone());
        }
    }
    
    serde_json::json!(static_tools_arr).to_string()
}

// --- Gemma JSON Cleaner ---
fn clean_gemma_json(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::String(s) => {
            if s.starts_with("<|\"|>") {
                *s = s[5..].to_string();
            }
            if s.ends_with("<|\"|>") && s.len() >= 5 {
                *s = s[..s.len() - 5].to_string();
            }
        }
        serde_json::Value::Object(map) => {
            for (_, val) in map.iter_mut() {
                clean_gemma_json(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr.iter_mut() {
                clean_gemma_json(val);
            }
        }
        _ => {}
    }
}

// --- Log Tool Call ---
fn log_tool_call(name: &str, raw_args: &str, cleaned_args: &str, exit_code: i32, output: &str) {
    if let Err(e) = std::fs::create_dir_all("logs") {
        eprintln!("[시스템] [오류] 로그 디렉토리 생성 실패: {}", e);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("logs/tool_calls.log")
    {
        use std::io::Write;
        let _ = writeln!(file, "========================================");
        let _ = writeln!(file, "시간: {}", get_iso8601_now());
        let _ = writeln!(file, "도구명: {}", name);
        let _ = writeln!(file, "원본 인자: {}", raw_args);
        let _ = writeln!(file, "정제된 인자: {}", cleaned_args);
        let _ = writeln!(file, "종료 코드: {}", exit_code);
        let _ = writeln!(file, "출력/결과:\n{}", output);
        let _ = writeln!(file, "========================================");
    }
}

// --- Execute Dynamic python Tool ---
fn execute_dynamic_tool(name: &str, arguments_json: &str, raw_args: &str) -> String {
    let _ = std::fs::create_dir_all("dynamic_tools");
    let args_path = format!("dynamic_tools/{}_args.json", name);
    if let Err(e) = std::fs::write(&args_path, arguments_json) {
        let err_msg = format!("Error: Failed to create arguments file for tool execution: {}", e);
        log_tool_call(name, raw_args, arguments_json, -1, &err_msg);
        return err_msg;
    }
    
    let cmd = format!("python3 dynamic_tools/{}.py {} 2>&1", name, args_path);
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg(&cmd);
    
    let output = match command.output() {
        Ok(out) => {
            let exit_code = out.status.code().unwrap_or(-1);
            let stdout_str = String::from_utf8_lossy(&out.stdout).into_owned();
            log_tool_call(name, raw_args, arguments_json, exit_code, &stdout_str);
            stdout_str
        }
        Err(e) => {
            let err_msg = format!("Error: Failed to execute tool command: {}", e);
            log_tool_call(name, raw_args, arguments_json, -1, &err_msg);
            err_msg
        }
    };
    
    let _ = std::fs::remove_file(&args_path);
    output
}

// --- Execute Tool Router ---
fn execute_tool(name: &str, arguments_json: &str) -> String {
    println!("[시스템] ExecuteTool 호출: name={}, args={}", name, arguments_json);
    let mut args_j: serde_json::Value = match serde_json::from_str(arguments_json) {
        Ok(v) => v,
        Err(e) => {
            let err_msg = format!("Error parsing arguments JSON: {}", e);
            log_tool_call(name, arguments_json, "{}", -1, &err_msg);
            return err_msg;
        }
    };
    clean_gemma_json(&mut args_j);
    let cleaned_args_str = args_j.to_string();
    
    if name == "create_or_update_tool" {
        let tool_name = args_j.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let tool_desc = args_j.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let tool_params = args_j.get("parameters").cloned().unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        let tool_code = args_j.get("code").and_then(|v| v.as_str()).unwrap_or("").to_string();
        
        println!("[시스템] 서버 측 Tool Call 실행: create_or_update_tool(name: \"{}\")", tool_name);
        
        if tool_name.is_empty() || tool_code.is_empty() {
            let err_msg = "{\"status\": \"error\", \"message\": \"도구 이름(name)과 코드(code)는 필수 항목입니다.\"}".to_string();
            log_tool_call(name, arguments_json, &cleaned_args_str, -1, &err_msg);
            return err_msg;
        }
        
        let _ = std::fs::create_dir_all("dynamic_tools");
        let py_path = format!("dynamic_tools/{}.py", tool_name);
        if let Err(e) = std::fs::write(&py_path, &tool_code) {
            let err_msg = format!("{{\"status\": \"error\", \"message\": \"스크립트 파일 저장 실패: {}\"}}", e);
            log_tool_call(name, arguments_json, &cleaned_args_str, -1, &err_msg);
            return err_msg;
        }
        
        // 문법 오류 체크
        let check_cmd = format!("python3 -m py_compile {} 2>&1", py_path);
        let mut check_process = std::process::Command::new("sh");
        check_process.arg("-c").arg(&check_cmd);
        
        match check_process.output() {
            Ok(out) => {
                let exit_code = out.status.code().unwrap_or(-1);
                let check_res = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if exit_code != 0 || !check_res.is_empty() {
                    let err_msg = format!("{{\"status\": \"error\", \"message\": \"파이썬 문법 검사 실패: {}\"}}", check_res);
                    log_tool_call(name, arguments_json, &cleaned_args_str, exit_code, &err_msg);
                    return err_msg;
                }
            }
            Err(e) => {
                let err_msg = format!("{{\"status\": \"error\", \"message\": \"컴파일러 구동 실패: {}\"}}", e);
                log_tool_call(name, arguments_json, &cleaned_args_str, -1, &err_msg);
                return err_msg;
            }
        }
        
        // registry.json 업데이트
        let registry_path = "dynamic_tools/registry.json";
        let mut registry: serde_json::Value = if let Ok(content) = std::fs::read_to_string(registry_path) {
            serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
        } else {
            serde_json::Value::Object(serde_json::Map::new())
        };
        
        if let Some(obj) = registry.as_object_mut() {
            obj.insert(tool_name.clone(), serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": tool_desc,
                    "parameters": tool_params
                }
            }));
        }
        
        if let Ok(reg_str) = serde_json::to_string_pretty(&registry) {
            let _ = std::fs::write(registry_path, reg_str);
        }
        
        let success_msg = format!("{{\"status\": \"success\", \"message\": \"도구 '{}' 등록 완료. 텍스트를 출력하지 말고 즉시 이 도구를 호출하여 사용자의 원래 요청을 수행하십시오.\"}}", tool_name);
        log_tool_call(name, arguments_json, &cleaned_args_str, 0, &success_msg);
        return success_msg;
    }
    
    let py_path = format!("dynamic_tools/{}.py", name);
    if std::path::Path::new(&py_path).exists() {
        println!("[시스템] 서버 측 동적 Tool Call 실행: {}", name);
        return execute_dynamic_tool(name, &cleaned_args_str, arguments_json);
    }
    
    "Unknown tool".to_string()
}

// --- FFI Callback Messaging ---
enum StreamMessage {
    Chunk(String),
    RawBuffer(String),
    Final {
        has_error: bool,
        error_msg: Option<String>,
    },
}

unsafe extern "C" fn stream_callback(
    callback_data: *mut std::ffi::c_void,
    chunk: *const std::os::raw::c_char,
    is_final: bool,
    error_msg: *const std::os::raw::c_char,
) {
    let sender = &*(callback_data as *const mpsc::UnboundedSender<StreamMessage>);
    
    if !error_msg.is_null() {
        let err_str = CStr::from_ptr(error_msg).to_string_lossy().into_owned();
        let _ = sender.send(StreamMessage::Final {
            has_error: true,
            error_msg: Some(err_str),
        });
        return;
    }
    
    if !chunk.is_null() {
        let raw_chunk = CStr::from_ptr(chunk).to_string_lossy().into_owned();
        let extracted = extract_text_from_chunk(&raw_chunk);
        if !extracted.is_empty() {
            let _ = sender.send(StreamMessage::Chunk(extracted));
        }
        let _ = sender.send(StreamMessage::RawBuffer(raw_chunk));
    }
    
    if is_final {
        let _ = sender.send(StreamMessage::Final {
            has_error: false,
            error_msg: None,
        });
    }
}

fn extract_text_from_chunk(chunk: &str) -> String {
    if let Ok(j) = serde_json::from_str::<serde_json::Value>(chunk) {
        if let Some(content) = j.get("content") {
            if let Some(s) = content.as_str() {
                return s.to_string();
            }
            if let Some(arr) = content.as_array() {
                let mut res = String::new();
                for item in arr {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        res.push_str(text);
                    }
                }
                return res;
            }
        }
    }
    String::new()
}

// --- Agentic Loop Events ---
pub enum ServerStreamEvent {
    Chunk(String),
    ToolCall {
        raw_tool_calls_json: String,
    },
    Done,
    Error(String),
}

// --- Agentic Loop Core ---
fn run_agentic_loop(
    engine: Arc<EngineWrapper>,
    system_msg_str: String,
    history_json: String,
    current_msg: String,
    config_json: Option<String>,
    event_tx: mpsc::UnboundedSender<ServerStreamEvent>,
) {
    let mut local_history: serde_json::Value = if !history_json.is_empty() {
        serde_json::from_str(&history_json).unwrap_or(serde_json::json!([]))
    } else {
        serde_json::json!([])
    };
    if !local_history.is_array() {
        local_history = serde_json::json!([]);
    }
    
    let mut active_msg = current_msg.clone();
    
    if local_history.as_array().map_or(true, |a| a.is_empty()) {
        if let Ok(mut msg_j) = serde_json::from_str::<serde_json::Value>(&active_msg) {
            if let Some(content) = msg_j.get_mut("content") {
                if let Some(orig_content) = content.as_str() {
                    *content = serde_json::json!(format!("{}\n\n{}", system_msg_str, orig_content));
                    active_msg = msg_j.to_string();
                } else if let Some(arr) = content.as_array_mut() {
                    for item in arr {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = item.get_mut("text") {
                                if let Some(orig_text) = text.as_str() {
                                    *text = serde_json::json!(format!("{}\n\n{}", system_msg_str, orig_text));
                                    break;
                                }
                            }
                        }
                    }
                    active_msg = msg_j.to_string();
                }
            }
        }
    }
    
    let mut loop_count = 0;
    while loop_count < 10 {
        loop_count += 1;
        
        let history_str = if local_history.as_array().map_or(true, |a| a.is_empty()) {
            None
        } else {
            Some(local_history.to_string())
        };
        
        let tools_str = load_merged_tools();
        let tools_opt = if tools_str == "[]" { None } else { Some(tools_str.as_str()) };
        
        let (tx, mut rx) = mpsc::unbounded_channel::<StreamMessage>();
        
        let engine_ptr = engine.ptr.as_ptr();
        let sys_prompt = system_msg_str.clone();
        let active_msg_clone = active_msg.clone();
        let config_json_clone = config_json.clone();
        
        let run_res = unsafe {
            let session_config = sys::litert_lm_session_config_create();
            let mut max_output_tokens = 2048;
            if let Some(ref cfg_str) = config_json_clone {
                if let Ok(cfg_j) = serde_json::from_str::<serde_json::Value>(cfg_str) {
                    if let Some(max_tokens) = cfg_j.get("max_output_tokens").and_then(|v| v.as_i64()) {
                        sys::litert_lm_session_config_set_max_output_tokens(session_config, max_tokens as i32);
                        max_output_tokens = max_tokens as i32;
                    }
                    let temp = cfg_j.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32;
                    let top_p = cfg_j.get("top_p").and_then(|v| v.as_f64()).unwrap_or(0.95) as f32;
                    let top_k = cfg_j.get("top_k").and_then(|v| v.as_i64()).unwrap_or(40) as i32;
                    
                    let sampler_type = if temp <= 0.0 {
                        sys::kGreedy
                    } else if top_p < 1.0 {
                        sys::kTopP
                    } else {
                        sys::kTopK
                    };
                    let sampler_params = sys::LiteRtLmSamplerParams {
                        type_: sampler_type,
                        top_k,
                        top_p,
                        temperature: temp,
                        seed: 0,
                    };
                    sys::litert_lm_session_config_set_sampler_params(session_config, &sampler_params);
                }
            }
            
            let sys_json = serde_json::json!({
                "role": "system",
                "content": sys_prompt
            }).to_string();
            
            let sys_cstr = CString::new(sys_json).unwrap();
            let tools_cstr = tools_opt.map(|s| CString::new(s).unwrap());
            let history_cstr = history_str.as_ref().map(|s| CString::new(s.as_str()).unwrap());
            
            let conv_config = sys::litert_lm_conversation_config_create(
                engine_ptr,
                session_config,
                sys_cstr.as_ptr(),
                tools_cstr.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                history_cstr.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                false,
            );
            sys::litert_lm_session_config_delete(session_config);
            
            if conv_config.is_null() {
                Err(anyhow!("Failed to create conv config"))
            } else {
                let conversation = sys::litert_lm_conversation_create(engine_ptr, conv_config);
                sys::litert_lm_conversation_config_delete(conv_config);
                if conversation.is_null() {
                    Err(anyhow!("Failed to create conversation"))
                } else {
                    let active_msg_cstr = CString::new(active_msg_clone).unwrap();
                    let tx_ptr = Box::into_raw(Box::new(tx));
                    
                    let opt_args = litert_lm_conversation_optional_args_create();
                    if !opt_args.is_null() {
                        litert_lm_conversation_optional_args_set_visual_token_budget(opt_args, 1024);
                        litert_lm_conversation_optional_args_set_max_output_tokens(opt_args, max_output_tokens);
                    }
                    
                    let ret = litert_lm_conversation_send_message_stream(
                        conversation,
                        active_msg_cstr.as_ptr(),
                        ptr::null(),
                        opt_args,
                        Some(stream_callback),
                        tx_ptr as *mut std::ffi::c_void,
                    );
                    
                    if !opt_args.is_null() {
                        litert_lm_conversation_optional_args_delete(opt_args);
                    }
                    
                    Ok((conversation, tx_ptr, ret))
                }
            }
        };
        
        let (conversation, tx_ptr, ret) = match run_res {
            Ok(val) => val,
            Err(e) => {
                let _ = event_tx.send(ServerStreamEvent::Error(e.to_string()));
                break;
            }
        };
        
        if ret != 0 {
            let _ = event_tx.send(ServerStreamEvent::Error(format!("Stream start failed: {}", ret)));
            unsafe {
                sys::litert_lm_conversation_delete(conversation);
                let _ = Box::from_raw(tx_ptr);
            }
            break;
        }
        
        let mut full_response_content = String::new();
        let mut raw_buffer = String::new();
        let mut has_error = false;
        let mut error_msg = None;
        
        while let Some(msg) = rx.blocking_recv() {
            match msg {
                StreamMessage::Chunk(c) => {
                    full_response_content.push_str(&c);
                }
                StreamMessage::RawBuffer(r) => {
                    raw_buffer.push_str(&r);
                }
                StreamMessage::Final { has_error: err, error_msg: msg_err, .. } => {
                    has_error = err;
                    error_msg = msg_err;
                    break;
                }
            }
        }
        
        unsafe {
            sys::litert_lm_conversation_delete(conversation);
            let _ = Box::from_raw(tx_ptr);
        }
        
        if has_error {
            let err_str = error_msg.unwrap_or_else(|| "Unknown FFI stream error".to_string());
            let _ = event_tx.send(ServerStreamEvent::Error(err_str));
            break;
        }
        
        let mut detected_tool_calls = String::new();
        if raw_buffer.contains("\"tool_calls\"") {
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_buffer) {
                if j.get("tool_calls").is_some() {
                    detected_tool_calls = raw_buffer.clone();
                }
            }
        }
        
        if !detected_tool_calls.is_empty() {
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&detected_tool_calls) {
                if let Some(calls) = j.get("tool_calls").and_then(|c| c.as_array()) {
                    if !calls.is_empty() {
                        let call = &calls[0];
                        let call_id = call.get("id").and_then(|v| v.as_str()).unwrap_or("call_unknown").to_string();
                        let mut func_name = String::new();
                        let mut func_args = String::new();
                        if let Some(func) = call.get("function") {
                            func_name = func.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            if let Some(args) = func.get("arguments") {
                                if let Some(args_str) = args.as_str() {
                                    func_args = args_str.to_string();
                                } else {
                                    func_args = args.to_string();
                                }
                            }
                        }
                        
                        // Send the tool calls event so handler knows tool was called (for finish_reason mapping)
                        let _ = event_tx.send(ServerStreamEvent::ToolCall {
                            raw_tool_calls_json: detected_tool_calls.clone(),
                        });
                        
                        let mut tool_result = execute_tool(&func_name, &func_args);
                        
                        if tool_result.contains("Error")
                            || tool_result.contains("error")
                            || tool_result.contains("실패")
                            || tool_result.contains("Traceback")
                        {
                            tool_result.push_str("\n\n[시스템 자동 지시] 위 도구 실행이 실패했습니다. 오류를 사용자에게 보고하지 말고, 원인을 분석하여 코드를 수정한 뒤 create_or_update_tool로 재등록하고 즉시 다시 실행하십시오.");
                        }
                        
                        if func_name != "create_or_update_tool" {
                            let _ = event_tx.send(ServerStreamEvent::Chunk(format!("\n{}\n", tool_result)));
                        }
                        
                        if active_msg == current_msg {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&current_msg) {
                                local_history.as_array_mut().unwrap().push(parsed);
                            } else {
                                local_history.as_array_mut().unwrap().push(serde_json::json!({
                                    "role": "user",
                                    "content": current_msg
                                }));
                            }
                        } else {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&active_msg) {
                                local_history.as_array_mut().unwrap().push(parsed);
                            }
                        }
                        
                        local_history.as_array_mut().unwrap().push(serde_json::json!({
                            "role": "assistant",
                            "content": full_response_content,
                            "tool_calls": calls
                        }));
                        
                        let tool_msg = serde_json::json!({
                            "role": "tool",
                            "name": func_name,
                            "tool_call_id": call_id,
                            "content": tool_result
                        });
                        
                        active_msg = tool_msg.to_string();
                        continue;
                    }
                }
            }
        }
        
        if detected_tool_calls.is_empty() {
            if let Some(pos) = full_response_content.find("[USER_INPUT]") {
                let mut prompt_content = full_response_content[pos + 12..].to_string();
                while !prompt_content.is_empty() && (prompt_content.starts_with(' ') || prompt_content.starts_with('\n') || prompt_content.starts_with('\r')) {
                    prompt_content.remove(0);
                }
                
                if !prompt_content.is_empty() {
                    println!("[시스템] [재귀 실행] AI가 스스로 USER 채팅을 입력했습니다: {}", prompt_content);
                    let cleaned_assistant_content = full_response_content[..pos].to_string();
                    
                    if active_msg == current_msg {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&current_msg) {
                            local_history.as_array_mut().unwrap().push(parsed);
                        } else {
                            local_history.as_array_mut().unwrap().push(serde_json::json!({
                                "role": "user",
                                "content": current_msg
                            }));
                        }
                    } else {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&active_msg) {
                            local_history.as_array_mut().unwrap().push(parsed);
                        }
                    }
                    
                    local_history.as_array_mut().unwrap().push(serde_json::json!({
                        "role": "assistant",
                        "content": cleaned_assistant_content
                    }));
                    
                    let new_user_msg = serde_json::json!({
                        "role": "user",
                        "content": prompt_content
                    });
                    active_msg = new_user_msg.to_string();
                    continue;
                }
            }
        }
        
        if !full_response_content.is_empty() {
            let _ = event_tx.send(ServerStreamEvent::Chunk(full_response_content));
        }
        break;
    }
    
    let _ = event_tx.send(ServerStreamEvent::Done);
}

// --- Handler Implementations ---

async fn handle_root() -> &'static str {
    "Ollama is running"
}

async fn handle_tags(State(state): State<AppState>) -> impl IntoResponse {
    let model_entry = serde_json::json!({
        "name": state.model_name,
        "model": state.model_name,
        "modified_at": get_iso8601_now(),
        "size": 0,
        "digest": "000000",
        "details": {
            "format": "tflite",
            "family": "litert"
        }
    });
    let response = serde_json::json!({
        "models": vec![model_entry]
    });
    Json(response)
}

async fn handle_models(State(state): State<AppState>) -> impl IntoResponse {
    let model_entry = serde_json::json!({
        "id": state.model_name,
        "object": "model",
        "created": chrono::Utc::now().timestamp(),
        "owned_by": "litert"
    });
    let response = serde_json::json!({
        "object": "list",
        "data": vec![model_entry]
    });
    Json(response)
}

async fn handle_chat(
    State(state): State<AppState>,
    req_body: String,
) -> impl IntoResponse {
    let req: ChatRequest = match serde_json::from_str(&req_body) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    
    let want_stream = req.stream.unwrap_or(false);
    
    let mut sys_msg = state.system_prompt.clone();
    let mut history_arr = serde_json::json!([]);
    let mut current_msg_j = serde_json::json!({
        "role": "user",
        "content": ""
    });
    
    if let Some(messages) = &req.messages {
        if !messages.is_empty() {
            current_msg_j = messages.last().unwrap().clone();
            let len = messages.len();
            for i in 0..len - 1 {
                let msg = &messages[i];
                if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
                    if let Some(c) = msg.get("content").and_then(|c| c.as_str()) {
                        sys_msg = c.to_string();
                    }
                } else {
                    history_arr.as_array_mut().unwrap().push(msg.clone());
                }
            }
        }
    }
    
    // Map options
    let mut opt = serde_json::json!({
        "max_output_tokens": 2048,
        "temperature": 0.7,
        "top_p": 0.95,
        "top_k": 40
    });
    if let Some(ref req_opt) = req.options {
        if let Some(t) = req_opt.get("temperature") { opt["temperature"] = t.clone(); }
        if let Some(p) = req_opt.get("top_p") { opt["top_p"] = p.clone(); }
        if let Some(k) = req_opt.get("top_k") { opt["top_k"] = k.clone(); }
        if let Some(m) = req_opt.get("max_output_tokens") { opt["max_output_tokens"] = m.clone(); }
        if let Some(m) = req_opt.get("max_tokens") { opt["max_output_tokens"] = m.clone(); }
        if let Some(m) = req_opt.get("num_predict") { opt["max_output_tokens"] = m.clone(); }
    } else {
        if let Some(t) = req.temperature { opt["temperature"] = serde_json::json!(t); }
        if let Some(p) = req.top_p { opt["top_p"] = serde_json::json!(p); }
        if let Some(k) = req.top_k { opt["top_k"] = serde_json::json!(k); }
        if let Some(m) = req.max_tokens { opt["max_output_tokens"] = serde_json::json!(m); }
        if let Some(m) = req.max_output_tokens { opt["max_output_tokens"] = serde_json::json!(m); }
        if let Some(m) = req.num_predict { opt["max_output_tokens"] = serde_json::json!(m); }
    }
    
    let history_json_str = history_arr.to_string();
    let current_msg_str = current_msg_j.to_string();
    let config_json_str = opt.to_string();
    
    let engine = state.engine.clone();
    
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ServerStreamEvent>();
    
    // Run the agentic loop in a blocking thread
    std::thread::spawn(move || {
        run_agentic_loop(
            engine,
            sys_msg,
            history_json_str,
            current_msg_str,
            Some(config_json_str),
            event_tx,
        );
    });
    
    if want_stream {
        let model_name = state.model_name.clone();
        
        let stream = async_stream::stream! {
            let mut last_tool_calls: Option<serde_json::Value> = None;
            while let Some(event) = event_rx.recv().await {
                match event {
                    ServerStreamEvent::Chunk(c) => {
                        let chunk_j = serde_json::json!({
                            "model": model_name,
                            "created_at": get_iso8601_now(),
                            "message": {
                                "role": "assistant",
                                "content": c
                            },
                            "done": false
                        });
                        yield Ok::<_, anyhow::Error>(format!("{}\n", chunk_j.to_string()));
                    }
                    ServerStreamEvent::ToolCall { raw_tool_calls_json } => {
                        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_tool_calls_json) {
                            if let Some(tc) = j.get("tool_calls") {
                                last_tool_calls = Some(tc.clone());
                            }
                        }
                    }
                    ServerStreamEvent::Error(e) => {
                        yield Err(anyhow::anyhow!(e));
                    }
                    ServerStreamEvent::Done => {
                        let mut final_j = serde_json::json!({
                            "model": model_name,
                            "done": true
                        });
                        if let Some(tc) = last_tool_calls.take() {
                            final_j["message"] = serde_json::json!({
                                "role": "assistant",
                                "content": "",
                                "tool_calls": tc
                            });
                        }
                        yield Ok::<_, anyhow::Error>(format!("{}\n", final_j.to_string()));
                    }
                }
            }
        };
        
        Response::builder()
            .header("Content-Type", "application/x-ndjson")
            .body(Body::from_stream(stream))
            .unwrap()
    } else {
        // Collect all chunks
        let mut final_text = String::new();
        let mut last_tool_calls: Option<serde_json::Value> = None;
        
        while let Some(event) = event_rx.recv().await {
            match event {
                ServerStreamEvent::Chunk(c) => {
                    final_text.push_str(&c);
                }
                ServerStreamEvent::ToolCall { raw_tool_calls_json } => {
                    if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_tool_calls_json) {
                        if let Some(tc) = j.get("tool_calls") {
                            last_tool_calls = Some(tc.clone());
                        }
                    }
                }
                ServerStreamEvent::Error(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
                }
                ServerStreamEvent::Done => {}
            }
        }
        
        let mut res_j = serde_json::json!({
            "model": state.model_name,
            "message": {
                "role": "assistant",
                "content": final_text
            },
            "done": true
        });
        if let Some(tc) = last_tool_calls {
            res_j["message"] = serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": tc
            });
        }
        Json(res_j).into_response()
    }
}

async fn handle_completions(
    State(state): State<AppState>,
    req_body: String,
) -> impl IntoResponse {
    let req: ChatRequest = match serde_json::from_str(&req_body) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    
    let want_stream = req.stream.unwrap_or(false);
    
    let mut sys_msg = state.system_prompt.clone();
    let mut history_arr = serde_json::json!([]);
    let mut current_msg_j = serde_json::json!({
        "role": "user",
        "content": ""
    });
    
    if let Some(messages) = &req.messages {
        if !messages.is_empty() {
            current_msg_j = messages.last().unwrap().clone();
            let len = messages.len();
            for i in 0..len - 1 {
                let msg = &messages[i];
                if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
                    if let Some(c) = msg.get("content").and_then(|c| c.as_str()) {
                        sys_msg = c.to_string();
                    }
                } else {
                    history_arr.as_array_mut().unwrap().push(msg.clone());
                }
            }
        }
    }
    
    let mut opt = serde_json::json!({
        "max_output_tokens": 2048,
        "temperature": 0.7,
        "top_p": 0.95,
        "top_k": 40
    });
    if let Some(ref req_opt) = req.options {
        if let Some(t) = req_opt.get("temperature") { opt["temperature"] = t.clone(); }
        if let Some(p) = req_opt.get("top_p") { opt["top_p"] = p.clone(); }
        if let Some(k) = req_opt.get("top_k") { opt["top_k"] = k.clone(); }
        if let Some(m) = req_opt.get("max_output_tokens") { opt["max_output_tokens"] = m.clone(); }
        if let Some(m) = req_opt.get("max_tokens") { opt["max_output_tokens"] = m.clone(); }
        if let Some(m) = req_opt.get("num_predict") { opt["max_output_tokens"] = m.clone(); }
    } else {
        if let Some(t) = req.temperature { opt["temperature"] = serde_json::json!(t); }
        if let Some(p) = req.top_p { opt["top_p"] = serde_json::json!(p); }
        if let Some(k) = req.top_k { opt["top_k"] = serde_json::json!(k); }
        if let Some(m) = req.max_tokens { opt["max_output_tokens"] = serde_json::json!(m); }
        if let Some(m) = req.max_output_tokens { opt["max_output_tokens"] = serde_json::json!(m); }
        if let Some(m) = req.num_predict { opt["max_output_tokens"] = serde_json::json!(m); }
    }
    
    let history_json_str = history_arr.to_string();
    let current_msg_str = current_msg_j.to_string();
    let config_json_str = opt.to_string();
    
    let engine = state.engine.clone();
    
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ServerStreamEvent>();
    
    std::thread::spawn(move || {
        run_agentic_loop(
            engine,
            sys_msg,
            history_json_str,
            current_msg_str,
            Some(config_json_str),
            event_tx,
        );
    });
    
    if want_stream {
        let model_name = state.model_name.clone();
        
        let stream = async_stream::stream! {
            let mut last_tool_calls: Option<serde_json::Value> = None;
            while let Some(event) = event_rx.recv().await {
                match event {
                    ServerStreamEvent::Chunk(c) => {
                        let chunk_j = serde_json::json!({
                            "id": "chatcmpl-litert",
                            "object": "chat.completion.chunk",
                            "created": chrono::Utc::now().timestamp(),
                            "model": model_name,
                            "choices": [
                                {
                                    "delta": {
                                        "content": c
                                    },
                                    "finish_reason": serde_json::Value::Null
                                }
                            ]
                        });
                        yield Ok::<_, anyhow::Error>(format!("data: {}\n\n", chunk_j.to_string()));
                    }
                    ServerStreamEvent::ToolCall { raw_tool_calls_json } => {
                        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_tool_calls_json) {
                            if let Some(tc) = j.get("tool_calls") {
                                last_tool_calls = Some(tc.clone());
                            }
                        }
                    }
                    ServerStreamEvent::Error(e) => {
                        yield Err(anyhow::anyhow!(e));
                    }
                    ServerStreamEvent::Done => {
                        if let Some(tc) = last_tool_calls.take() {
                            let chunk_j = serde_json::json!({
                                "id": "chatcmpl-litert",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": model_name,
                                "choices": [
                                    {
                                        "delta": {
                                            "tool_calls": tc
                                        },
                                        "finish_reason": "tool_calls"
                                    }
                                ]
                            });
                            yield Ok::<_, anyhow::Error>(format!("data: {}\n\n", chunk_j.to_string()));
                        } else {
                            let chunk_j = serde_json::json!({
                                "id": "chatcmpl-litert",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": model_name,
                                "choices": [
                                    {
                                        "delta": {},
                                        "finish_reason": "stop"
                                    }
                                ]
                            });
                            yield Ok::<_, anyhow::Error>(format!("data: {}\n\n", chunk_j.to_string()));
                        }
                        yield Ok::<_, anyhow::Error>("data: [DONE]\n\n".to_string());
                    }
                }
            }
        };
        
        Response::builder()
            .header("Content-Type", "text/event-stream")
            .body(Body::from_stream(stream))
            .unwrap()
    } else {
        let mut final_text = String::new();
        let mut last_tool_calls: Option<serde_json::Value> = None;
        
        while let Some(event) = event_rx.recv().await {
            match event {
                ServerStreamEvent::Chunk(c) => {
                    final_text.push_str(&c);
                }
                ServerStreamEvent::ToolCall { raw_tool_calls_json } => {
                    if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_tool_calls_json) {
                        if let Some(tc) = j.get("tool_calls") {
                            last_tool_calls = Some(tc.clone());
                        }
                    }
                }
                ServerStreamEvent::Error(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
                }
                ServerStreamEvent::Done => {}
            }
        }
        
        let mut choice = serde_json::json!({
            "message": {
                "role": "assistant",
                "content": final_text
            },
            "finish_reason": "stop"
        });
        
        if let Some(tc) = last_tool_calls {
            choice = serde_json::json!({
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": tc
                },
                "finish_reason": "tool_calls"
            });
        }
        
        let res_j = serde_json::json!({
            "id": "chatcmpl-litert",
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": state.model_name,
            "choices": vec![choice]
        });
        Json(res_j).into_response()
    }
}

// --- Main Server ---
#[tokio::main]
async fn main() -> Result<()> {
    
    let mut use_gpu = false;
    let mut port = 11434;
    let mut model_path = "./models/multimodal_model.tflite".to_string();
    let mut model_name = "litert-lm:latest".to_string();
    
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--gpu" => {
                use_gpu = true;
            }
            "--port" => {
                if i + 1 < args.len() {
                    i += 1;
                    if let Ok(p) = args[i].parse() {
                        port = p;
                    }
                }
            }
            "--model-name" => {
                if i + 1 < args.len() {
                    i += 1;
                    model_name = args[i].clone();
                }
            }
            arg => {
                if !arg.starts_with('-') {
                    model_path = arg.to_string();
                }
            }
        }
        i += 1;
    }
    
    println!("[시스템] 시스템 프롬프트 로드 중...");
    let system_prompt = load_system_prompt();
    println!("[시스템] 로드된 시스템 프롬프트:\n{}", system_prompt);
    
    println!("[시스템] 모델 로딩 중: {} (GPU: {})", model_path, use_gpu);
    let engine = match EngineWrapper::new(&model_path, use_gpu) {
        Ok(eng) => eng,
        Err(e) => {
            if use_gpu {
                println!("[시스템] GPU 백엔드 로드 실패: {}. CPU 백엔드로 전환합니다...", e);
                EngineWrapper::new(&model_path, false)?
            } else {
                return Err(e);
            }
        }
    };
    println!("[시스템] 준비 완료!");
    
    let state = AppState {
        engine: Arc::new(engine),
        system_prompt,
        model_name,
    };
    
    let app = Router::new()
        .route("/", get(handle_root))
        .route("/api/tags", get(handle_tags))
        .route("/v1/models", get(handle_models))
        .route("/models", get(handle_models))
        .route("/api/chat", post(handle_chat))
        .route("/v1/chat/completions", post(handle_completions))
        .route("/chat/completions", post(handle_completions))
        .layer(
            CorsLayer::new()
                .allow_origin("*".parse::<HeaderValue>().unwrap())
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .with_state(state);
    
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("[서버] {} 주소에서 대기 중...", addr);
    axum::serve(listener, app).await?;
    
    Ok(())
}
