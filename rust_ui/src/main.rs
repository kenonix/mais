use clap::Parser;
use colored::*;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Write, stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

// --- 기본 AI 영혼 정의 (soul.txt 미존재 시 펴백) ---
const DEFAULT_SOUL: &str = r#"당신의 이름은 AI입니다.
한국어만 사용하며, 친절하고 명확하게 답변합니다.
수학적 그래프 시각화가 필요할 경우 반드시 ```latex 수식 ``` 블록을 사용하세요."#;

/// soul.txt + tools.txt를 합산하여 하나의 시스템 프롬프트로 반환
fn load_system_prompt() -> String {
    // soul.txt 로드 (상위 디렉토리 → 현재 디렉토리)
    let soul = ["../soul.txt", "soul.txt"]
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok())
        .find(|c| !c.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SOUL.to_string());

    // tools.txt 로드
    let tools = ["../tools.txt", "tools.txt"]
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok())
        .find(|c| !c.trim().is_empty())
        .unwrap_or_default();

    if tools.is_empty() {
        soul.trim().to_string()
    } else {
        format!("{}\n\n{}", soul.trim(), tools.trim())
    }
}

// --- CLI 인자 정의 ---
#[derive(Parser, Debug)]
#[command(author, version, about = "LiteRT-LM Multimodal Chat Client", long_about = None)]
struct Args {
    /// 서버 호스트 주소
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// 서버 포트 번호
    #[arg(long, default_value = "11434")]
    port: u16,
}

// --- 채팅 설정 구조체 ---
#[derive(Clone, Serialize, Deserialize)]
struct ChatSettings {
    temperature: f32,
    top_p: f32,
    top_k: u32,
    max_tokens: u32,
    system_prompt: String,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.95,
            top_k: 40,
            max_tokens: 2048,
            system_prompt: load_system_prompt(),
        }
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// 설정 파일 관리 (soul.txt, tools.txt, config.json)
// ─────────────────────────────────────────────────────────────────────────────

/// 설정 파일 로드 (상위 디렉토리 → 현재 디렉토리 순 탐색)
fn load_settings() -> ChatSettings {
    let mut settings = ChatSettings::default();

    // 생성 옵션 로드
    let config_paths = ["../config.json", "config.json"];
    for path in &config_paths {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(j) = serde_json::from_str::<Value>(&content) {
                parse_config_json(&mut settings, &j);
                break;
            }
        }
    }

    settings
}

fn parse_config_json(settings: &mut ChatSettings, j: &Value) {
    if let Some(t) = j.get("temperature").and_then(|v| v.as_f64()) {
        settings.temperature = t as f32;
    }
    if let Some(p) = j.get("top_p").and_then(|v| v.as_f64()) {
        settings.top_p = p as f32;
    }
    if let Some(k) = j.get("top_k").and_then(|v| v.as_u64()) {
        settings.top_k = k as u32;
    }
    if let Some(m) = j.get("max_output_tokens").and_then(|v| v.as_u64()) {
        settings.max_tokens = m as u32;
    }
}

fn save_settings(settings: &ChatSettings) {
    // soul.txt만 저장 (tools.txt는 사용자가 직접 편집)
    let _ = fs::write("soul.txt", &settings.system_prompt);
    let config_j = json!({
        "temperature": settings.temperature,
        "top_p": settings.top_p,
        "top_k": settings.top_k,
        "max_output_tokens": settings.max_tokens
    });
    if let Ok(formatted) = serde_json::to_string_pretty(&config_j) {
        let _ = fs::write("config.json", formatted);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 입력 보조 유틸리티
// ─────────────────────────────────────────────────────────────────────────────

/// ~ 경로를 절대 홈 경로로 확장
fn expand_path(path_str: &str) -> String {
    let trimmed = path_str.trim();
    if trimmed.starts_with('~') {
        if let Some(home) = std::env::var_os("HOME") {
            let mut p = PathBuf::from(home);
            if trimmed.len() > 1 {
                p.push(&trimmed[2..]);
            }
            return p.to_string_lossy().to_string();
        }
    }
    Path::new(trimmed).to_string_lossy().to_string()
}

/// 설정 항목 대화형 입력 도우미
fn prompt_input(label: &str, current: &str) -> Option<String> {
    print!("{} [{}] ❯ ", label, current);
    let _ = stdout().flush();
    let mut val = String::new();
    let _ = io::stdin().read_line(&mut val);
    let trimmed = val.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

// ─────────────────────────────────────────────────────────────────────────────
// 비동기 스피너 (대기 애니메이션)
// ─────────────────────────────────────────────────────────────────────────────
struct Spinner {
    stop_tx: Option<mpsc::Sender<()>>,
}

impl Spinner {
    fn start(text: &'static str) -> Self {
        let (tx, mut rx) = mpsc::channel(1);
        tokio::spawn(async move {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i = 0;
            loop {
                tokio::select! {
                    _ = rx.recv() => {
                        print!("\r\x1B[K");
                        let _ = stdout().flush();
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(80)) => {
                        print!("\r{} {}", frames[i % frames.len()].cyan().bold(), text.dimmed());
                        let _ = stdout().flush();
                        i += 1;
                    }
                }
            }
        });
        Self { stop_tx: Some(tx) }
    }

    fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.try_send(());
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 메인 클라이언트 루프
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let client = Client::new();
    let server_url = format!("http://{}:{}", args.host, args.port);

    // 헤더 배너 출력
    println!("{}", "═══════════════════════════════════════════════════".blue());
    println!("{}", "         ✨ LiteRT-LM Multimodal Chat Client ✨    ".bold().cyan());
    println!("{}", "═══════════════════════════════════════════════════".blue());
    println!("  • /clear        : 대화 기록 초기화");
    println!("  • /img <경로>   : 이미지 첨부 (멀티모달)");
    println!("  • /settings     : 파라미터 및 시스템 프롬프트 설정");
    println!("  • /exit         : 프로그램 종료");
    println!("{}\n", "═══════════════════════════════════════════════════".blue());

    let mut settings = load_settings();
    let mut history: Vec<Value> = Vec::new();
    let mut pending_image: Option<String> = None;

    loop {
        // 첨부 이미지 인디케이터
        if let Some(ref img_path) = pending_image {
            print!("{} ", format!("[첨부 이미지: {}]", img_path).yellow().bold());
        }

        print!("{} ❯ ", "User".cyan().bold());
        let _ = stdout().flush();

        let mut user_input = String::new();
        io::stdin().read_line(&mut user_input)?;
        let user_input = user_input.trim();

        // 커맨드 처리
        match user_input {
            "/exit" | "/quit" => {
                println!("{}", "프로그램을 종료합니다.".red());
                break;
            }
            "/clear" => {
                history.clear();
                pending_image = None;
                println!("{}", "💡 대화 기록과 이미지가 초기화되었습니다.".green().bold());
                continue;
            }
            "/settings" => {
                println!("\n{}", "⚙️  현재 설정 내역".bold().yellow());
                println!("  1. System Prompt: {}", settings.system_prompt.dimmed());
                println!("  2. Temperature  : {}", settings.temperature);
                println!("  3. Top-P        : {}", settings.top_p);
                println!("  4. Top-K        : {}", settings.top_k);
                println!("  5. Max Tokens   : {}", settings.max_tokens);
                print!("\n설정을 변경하시겠습니까? (y/N) ❯ ");
                let _ = stdout().flush();
                let mut choice = String::new();
                io::stdin().read_line(&mut choice)?;

                if choice.trim().eq_ignore_ascii_case("y") {
                    if let Some(v) = prompt_input("System Prompt", &settings.system_prompt) {
                        settings.system_prompt = v;
                    }
                    if let Some(v) = prompt_input("Temperature", &settings.temperature.to_string()) {
                        if let Ok(t) = v.parse::<f32>() { settings.temperature = t; }
                    }
                    if let Some(v) = prompt_input("Top-P", &settings.top_p.to_string()) {
                        if let Ok(p) = v.parse::<f32>() { settings.top_p = p; }
                    }
                    if let Some(v) = prompt_input("Top-K", &settings.top_k.to_string()) {
                        if let Ok(k) = v.parse::<u32>() { settings.top_k = k; }
                    }
                    if let Some(v) = prompt_input("Max Tokens", &settings.max_tokens.to_string()) {
                        if let Ok(m) = v.parse::<u32>() { settings.max_tokens = m; }
                    }
                    save_settings(&settings);
                    println!("{}", "💾 설정이 저장되었습니다.".green().bold());
                }
                continue;
            }
            cmd if cmd.starts_with("/img ") => {
                let resolved_path = expand_path(&cmd[5..]);
                if Path::new(&resolved_path).exists() {
                    pending_image = Some(resolved_path.clone());
                    println!("{} {}", "🖼 이미지 첨부 완료:".green().bold(), resolved_path.yellow());
                } else {
                    println!("{} {}", "❌ 파일을 찾을 수 없습니다:".red().bold(), resolved_path.red());
                }
                continue;
            }
            "" if pending_image.is_none() => continue,
            _ => {}
        }

        // 메시지 구성 (텍스트 or 멀티모달)
        let content_val = match pending_image.take() {
            Some(img_path) => {
                let mut parts = vec![json!({"type": "image", "path": img_path})];
                if !user_input.is_empty() {
                    parts.push(json!({"type": "text", "text": user_input}));
                }
                Value::Array(parts)
            }
            None => Value::String(user_input.to_string()),
        };

        history.push(json!({"role": "user", "content": content_val}));

        // 도구 실행 루프 (재귀 호출 처리, 최대 10턴)
        let mut loop_count = 0;
        let mut run_chat = true;

        while run_chat && loop_count < 10 {
            loop_count += 1;
            run_chat = false;

            // 요청 메시지 어셈블리 (시스템 프롬프트 + 히스토리)
            let mut request_messages = vec![json!({
                "role": "system",
                "content": settings.system_prompt
            })];
            request_messages.extend(history.clone());

            let request_payload = json!({
                "model": "litert-lm:latest",
                "messages": request_messages,
                "stream": true,
                "options": {
                    "temperature": settings.temperature,
                    "top_p": settings.top_p,
                    "top_k": settings.top_k,
                    "max_output_tokens": settings.max_tokens
                }
            });

            let mut spinner = Spinner::start("AI 생각 중...");

            let response = match client.post(format!("{}/api/chat", server_url))
                .json(&request_payload)
                .send()
                .await
            {
                Ok(resp) => { spinner.stop(); resp }
                Err(e) => {
                    spinner.stop();
                    println!("\n{} {}", "❌ 서버 통신 오류:".red().bold(), e);
                    break;
                }
            };

            if !response.status().is_success() {
                let err_text = response.text().await.unwrap_or_default();
                println!("\n{} {}", "❌ 서버 에러 응답:".red().bold(), err_text);
                break;
            }

            print!("{} ❯ ", "AI".magenta().bold());
            let _ = stdout().flush();

            // 스트리밍 수신 및 출력
            let mut stream = response.bytes_stream();
            let mut full_response_content = String::new();
            let mut tool_calls: Option<Value> = None;
            let mut line_buffer = String::new();

            while let Some(chunk_res) = stream.next().await {
                let chunk = match chunk_res {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        println!("\n{} {}", "❌ 스트리밍 오류:".red().bold(), e);
                        break;
                    }
                };

                let chunk_str_dbg = String::from_utf8_lossy(&chunk);
                // eprintln!("[DEBUG CHUNK] {}", chunk_str_dbg);

                line_buffer.push_str(&chunk_str_dbg);
                while let Some(pos) = line_buffer.find('\n') {
                    let line = line_buffer[..pos].trim().to_string();
                    line_buffer.drain(..=pos);
                    
                    if line.is_empty() { continue; }
                    if let Ok(j) = serde_json::from_str::<Value>(&line) {
                        if let Some(msg) = j.get("message") {
                            // 도구 호출 감지
                            if let Some(calls) = msg.get("tool_calls") {
                                if !calls.is_null() && calls.as_array().map_or(false, |a| !a.is_empty()) {
                                    tool_calls = Some(calls.clone());
                                }
                            }
                            // 텍스트 출력
                            if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                                if !content.is_empty() {
                                    print!("{}", content);
                                    let _ = stdout().flush();
                                    full_response_content.push_str(content);
                                }
                            }
                        }
                    }
                }
            }
            println!();

            // 히스토리에 어시스턴트 응답 추가
            let mut assistant_message = json!({
                "role": "assistant",
                "content": full_response_content
            });
            if let Some(ref calls) = tool_calls {
                assistant_message["tool_calls"] = calls.clone();
            }
            history.push(assistant_message);

            // 도구 호출은 서버 측에서 처리되므로 클라이언트는 다음 턴만 대기
            // (서버가 도구 실행 결과를 히스토리에 넣고 다음 응답을 생성함)
            if tool_calls.is_some() {
                run_chat = true;
            }
        }
    }

    Ok(())
}
