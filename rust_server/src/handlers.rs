// Axum 웹 프레임워크에서 라우팅 핸들링 및 상태 공유를 위해 필요한 도구들을 가져옵니다.
use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
// JSON 직렬화/역직렬화에 대응하기 위해 serde 및 serde_json 패키지를 임포트합니다.
use serde;
use serde_json;
// 에이전트 스레드와 핸들러 스레드 간 비동기 메시지 중계를 위해 Tokio 채널을 준비합니다.
use tokio::sync::mpsc;
// main 모듈에 선언되어 있는 전역 AppState 설정을 가져옵니다.
use crate::AppState;
// 자율 에이전트 루프 및 이벤트 전송 규격을 가져옵니다.
use crate::agentic::{run_agentic_loop, ServerStreamEvent};
// 현재 시간 파싱용 유틸리티를 가져옵니다.
use crate::utils::get_iso8601_now;

// 클라이언트가 보낸 HTTP POST API 요청 바디 데이터를 파싱하기 위한 구조체 정의입니다.
#[derive(serde::Deserialize, Debug)]
pub struct ChatRequest {
    // 역대 전체 대화 메시지들이 들어있는 선택적 JSON 배열 필드입니다.
    pub messages: Option<Vec<serde_json::Value>>,
    // 결과를 한 번에 받을지 스트리밍할지 결정하는 선택적 불리언 스위치 필드입니다.
    pub stream: Option<bool>,
    // 온도를 포함하여 옵션 묶음 전체가 들어 있는 선택적 JSON 객체 필드입니다.
    pub options: Option<serde_json::Value>,
    // LLM 추론 온도 조절 하이퍼파라미터입니다.
    pub temperature: Option<f64>,
    // 누적 후보군 범위 조절용 탑피 계수입니다.
    pub top_p: Option<f64>,
    // 상위 후보군 선택 범위 조절용 탑케이 정수입니다.
    pub top_k: Option<i64>,
    // 생성 가능한 최고 토큰 길이에 대한 옵션 변수입니다.
    pub max_tokens: Option<i64>,
    // 완료 생성의 출력 한계 토큰 수 변수입니다.
    pub max_output_tokens: Option<i64>,
    // 예측할 최대 토큰 수 설정 옵션 변수입니다.
    pub num_predict: Option<i64>,
}

// 서버 루트 주소("/")에 대응하여 Ollama 호환성을 증명하는 단순 인사말 핸들러입니다.
pub async fn handle_root() -> &'static str {
    // Ollama가 잘 작동 중이라는 상태 텍스트를 고정 응답으로 보냅니다.
    "Ollama is running"
}

// Ollama API 규격에 맞춰 현재 탑재하여 서빙 중인 단일 모델 사양을 알려주는 tags 핸들러입니다.
pub async fn handle_tags(State(state): State<AppState>) -> impl IntoResponse {
    // 모델 세부 명세를 JSON 포맷에 맞추어 조립합니다.
    let model_entry = serde_json::json!({
        // 모델 이름 적용
        "name": state.model_name,
        // 모델명 명시
        "model": state.model_name,
        // 수정 시간 기록
        "modified_at": get_iso8601_now(),
        // 파일 용량 가상 표기
        "size": 0,
        // 검증 해시 가상 표기
        "digest": "000000",
        // 포맷 정보
        "details": {
            "format": "tflite",
            "family": "litert"
        }
    });
    // Ollama tags 응답 규격인 "models" 배열 구조에 담아서 래핑합니다.
    let response = serde_json::json!({
        "models": vec![model_entry]
    });
    // JSON 응답으로 응수합니다.
    Json(response)
}

// OpenAI 호환 모델 목록 조회 API("/v1/models")에 대응하는 핸들러입니다.
pub async fn handle_models(State(state): State<AppState>) -> impl IntoResponse {
    // OpenAI 규격에 맞춰서 현재 띄워져 작동 중인 단일 모델 카탈로그를 작성합니다.
    let model_entry = serde_json::json!({
        // 모델명 기입
        "id": state.model_name,
        // 객체 타입 명시
        "object": "model",
        // 생성 시간 세팅
        "created": chrono::Utc::now().timestamp(),
        // 소유 회사 표기
        "owned_by": "litert"
    });
    // OpenAI 응답 표준 패킷 형태인 "object"와 "data" 규격 맵으로 조립합니다.
    let response = serde_json::json!({
        "object": "list",
        "data": vec![model_entry]
    });
    // JSON 응답 발송
    Json(response)
}

// Ollama 규격 채팅 서비스("/api/chat")를 연동하고 자율 에이전트 루프와 매핑하는 메인 핸들러입니다.
pub async fn handle_chat(
    // 공유된 서버 앱 스테이트(AppState) 데이터들을 Axum 추출기로 받아옵니다.
    State(state): State<AppState>,
    // HTTP 요청 본문 원시 문자열 본문을 수거합니다.
    req_body: String,
) -> impl IntoResponse {
    // 수신한 JSON을 앞서 선언한 ChatRequest 규격 모델로 해독하며, 문법 붕괴 시 BAD_REQUEST(400)를 응답합니다.
    let req: ChatRequest = match serde_json::from_str(&req_body) {
        // 성공 시 데이터 인계
        Ok(r) => r,
        // 파싱 불능 시 400 상태코드와 에러 본문을 회신하고 조기 중단합니다.
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    
    // 스트리밍 출력을 선호하고 기입했는지 체크하며 없으면 비스트리밍(false)으로 선언합니다.
    let want_stream = req.stream.unwrap_or(false);
    
    // 시스템 성격을 서버 전역 지침서로부터 복사해와 초기화합니다.
    let mut sys_msg = state.system_prompt.clone();
    // 이전 대화 내역들을 보관할 JSON 빈 배열 객체를 준비합니다.
    let mut history_arr = serde_json::json!([]);
    // 이번 차례의 질문 내용을 기재할 기본 사용자 역할 포맷을 설정합니다.
    let mut current_msg_j = serde_json::json!({
        "role": "user",
        "content": ""
    });
    
    // 클라이언트가 역대 대화 배열을 전송해 넘겨준 상태가 감지될 때의 처리입니다.
    if let Some(messages) = &req.messages {
        // 메시지들이 존재하고 비어있지 않은 것을 검사합니다.
        if !messages.is_empty() {
            // 전체 배열의 가장 맨 마지막 요소가 바로 이번 턴의 신규 유저 질문이므로 추출 기재합니다.
            current_msg_j = messages.last().unwrap().clone();
            // 전체 메시지 개수를 계산합니다.
            let len = messages.len();
            // 마지막 질문을 제외한 0번째부터 직전 턴까지의 대화 배열을 루프 돌려 히스토리에 수납합니다.
            for i in 0..len - 1 {
                // 특정 순번의 메시지 수거
                let msg = &messages[i];
                // 역할이 "system"인지 조사하여 맞다면 최신 시스템 메시지로 주입 갱신합니다.
                if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
                    // 성격 문안을 교체
                    if let Some(c) = msg.get("content").and_then(|c| c.as_str()) {
                        // 내용물 추출 주입
                        sys_msg = c.to_string();
                    }
                } else {
                    // 시스템 메시지가 아니라면 일반 유저/어시스턴트 대화이므로 대화 목록 배열에 적재합니다.
                    history_arr.as_array_mut().unwrap().push(msg.clone());
                }
            }
        }
    }
    
    // 모델 세션 파라미터 제어를 위한 기본 하이퍼파라미터 양식을 세팅합니다.
    let mut opt = serde_json::json!({
        "max_output_tokens": 262144,
        "temperature": 0.7,
        "top_p": 0.95,
        "top_k": 40
    });
    // 상세 하이퍼파라미터 구조가 options 필드로 전송된 경우 해당 매핑을 오버라이딩합니다.
    if let Some(ref req_opt) = req.options {
        // 온도 변경 시 업데이트
        if let Some(t) = req_opt.get("temperature") { opt["temperature"] = t.clone(); }
        // 누적 확률 조정 시 반영
        if let Some(p) = req_opt.get("top_p") { opt["top_p"] = p.clone(); }
        // 후보군 크기 조정 시 반영
        if let Some(k) = req_opt.get("top_k") { opt["top_k"] = k.clone(); }
        // 출력 토큰 상한 제한 시 반영
        if let Some(m) = req_opt.get("max_output_tokens") { opt["max_output_tokens"] = m.clone(); }
        // 대체 변수 max_tokens 매핑 처리
        if let Some(m) = req_opt.get("max_tokens") { opt["max_output_tokens"] = m.clone(); }
        // 대체 변수 num_predict 매핑 처리
        if let Some(m) = req_opt.get("num_predict") { opt["max_output_tokens"] = m.clone(); }
    } else {
        // options 구조체 외부에 개별 필드로 전달되었을 때의 대안 매핑 분기입니다.
        if let Some(t) = req.temperature { opt["temperature"] = serde_json::json!(t); }
        // top_p 세팅
        if let Some(p) = req.top_p { opt["top_p"] = serde_json::json!(p); }
        // top_k 세팅
        if let Some(k) = req.top_k { opt["top_k"] = serde_json::json!(k); }
        // max_tokens 세팅
        if let Some(m) = req.max_tokens { opt["max_output_tokens"] = serde_json::json!(m); }
        // max_output_tokens 세팅
        if let Some(m) = req.max_output_tokens { opt["max_output_tokens"] = serde_json::json!(m); }
        // num_predict 세팅
        if let Some(m) = req.num_predict { opt["max_output_tokens"] = serde_json::json!(m); }
    }
    
    // 준비 완료된 각 파라미터를 에이전트 연산 스레드로 넘겨줄 수 있게 평문형 문자열로 인코딩합니다.
    let history_json_str = history_arr.to_string();
    // 현재 메시지 인코딩
    let current_msg_str = current_msg_j.to_string();
    // 옵션 세팅 인코딩
    let config_json_str = opt.to_string();
    
    // 엔진에 대한 참조 복제를 확보합니다.
    let engine = state.engine.clone();
    
    // 에이전트 연쇄 스레드로부터 비동기식 상태보고 이벤트를 접수할 MPSC 채널을 선언합니다.
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ServerStreamEvent>();
    
    // AI 추론 연산 처리가 메인 HTTP 서버 웹 핸들러를 Block하지 않도록 별도의 백그라운드 스레드에서 구동합니다.
    std::thread::spawn(move || {
        // 메인 에이전트 실행 루프를 호출하여 격발합니다.
        run_agentic_loop(
            // 엔진 전달
            engine,
            // 성격 프롬프트 전달
            sys_msg,
            // 대화 목록 전달
            history_json_str,
            // 현재 질문 전달
            current_msg_str,
            // 하이퍼파라미터 양식 전달
            Some(config_json_str),
            // 통지 소켓 송신기 이양
            event_tx,
        );
    });
    
    // 스트리밍 방식(Ollama용 NDJSON 스트림)을 원했을 경우의 리턴 처리 분기입니다.
    if want_stream {
        // 내부 모델 식별명 복제
        let model_name = state.model_name.clone();
        
        // async-stream 크레이트의 stream! 매크로 생성기를 통해 비동기 스트림 응답 제너레이터를 조립합니다.
        let stream = async_stream::stream! {
            // 연쇄 동작 중 튀어나왔던 가장 최근 도구 지시 코드를 추적 저장해둘 옵션 변수입니다.
            let mut last_tool_calls: Option<serde_json::Value> = None;
            // 채널 수신기(event_rx)로부터 새 보고 이벤트가 접수되는 동안 스트림 중계를 반복 진행합니다.
            while let Some(event) = event_rx.recv().await {
                // 수령된 통지 매칭
                match event {
                    // 일반 생성 텍스트 단편을 넘겨받은 상황
                    ServerStreamEvent::Chunk(c) => {
                        // Ollama 규격 채팅 스트림용 JSON 프레임을 구축합니다.
                        let chunk_j = serde_json::json!({
                            "model": model_name,
                            "created_at": get_iso8601_now(),
                            "message": {
                                "role": "assistant",
                                "content": c
                            },
                            "done": false
                        });
                        // NDJSON 규약에 맞춰 개행 문자(\n)와 함께 직렬화 문자열을 yield로 흘려보냅니다.
                        yield Ok::<_, anyhow::Error>(format!("{}\n", chunk_j.to_string()));
                    }
                    // 모델이 도구 가동 명령(tool_calls)을 격발한 경우의 스트림 중계입니다.
                    ServerStreamEvent::ToolCall { raw_tool_calls_json } => {
                        // 들어온 도구 지시 원시 JSON을 안전 해석해 봅니다.
                        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_tool_calls_json) {
                            // tool_calls 배열이 자리잡은 게 확인되면 데이터 가공을 합니다.
                            if let Some(tc) = j.get("tool_calls") {
                                // 최종 완료 통보 턴에 첨부하기 위해 변수에 기억 보존합니다.
                                last_tool_calls = Some(tc.clone());
                                // Ollama 도구 실행 공지 포맷에 맞춘 중간 통보 프레임을 형성합니다.
                                let chunk_j = serde_json::json!({
                                    "model": model_name,
                                    "created_at": get_iso8601_now(),
                                    "message": {
                                        "role": "assistant",
                                        "content": "",
                                        "tool_calls": tc
                                    },
                                    "done": false
                                });
                                // yield 출력
                                yield Ok::<_, anyhow::Error>(format!("{}\n", chunk_j.to_string()));
                            }
                        }
                    }
                    // 파이썬 연산 도구 실행 완료 데이터가 도착했을 때의 중계 처리입니다.
                    ServerStreamEvent::ToolResult { name, result } => {
                        // 도구 최종 아웃풋 내역을 지닌 임시 알림 NDJSON을 조립합니다.
                        let chunk_j = serde_json::json!({
                            "model": model_name,
                            "created_at": get_iso8601_now(),
                            "tool_result": {
                                "name": name,
                                "result": result
                            },
                            "done": false
                        });
                        // yield 출력
                        yield Ok::<_, anyhow::Error>(format!("{}\n", chunk_j.to_string()));
                    }
                    // 시스템 내부 공정 가이드라인(Guidance) 알림이 도착했을 때의 중계 처리입니다.
                    ServerStreamEvent::Guidance(g) => {
                        // 안내문 용도의 JSON 프레임을 빌드합니다.
                        let chunk_j = serde_json::json!({
                            "model": model_name,
                            "created_at": get_iso8601_now(),
                            "guidance": g,
                            "done": false
                        });
                        // yield 출력
                        yield Ok::<_, anyhow::Error>(format!("{}\n", chunk_j.to_string()));
                    }
                    // 처리 에러가 감지되었을 때 스트림 상으로 예외 에러를 반환 격발합니다.
                    ServerStreamEvent::Error(e) => {
                        // 스트림 에러 송출
                        yield Err(anyhow::anyhow!(e));
                    }
                    // 모든 에이전트 가동 체인이 마감되어 완료 신호(Done)가 뜬 상황입니다.
                    ServerStreamEvent::Done { final_history, prompt_tokens, completion_tokens } => {
                        // Ollama 스트림 최종 종결용 JSON 구조(done: true 포함)를 다져 넣습니다.
                        let mut final_j = serde_json::json!({
                            "model": model_name,
                            "done": true,
                            "history": final_history,
                            "prompt_tokens": prompt_tokens,
                            "completion_tokens": completion_tokens,
                            "total_tokens": prompt_tokens + completion_tokens
                        });
                        // 도구가 마지막으로 가동된 기록이 있다면 assistant 롤 규격 아래에 명기해 주입합니다.
                        if let Some(tc) = last_tool_calls.take() {
                            // 지시서 배열 머지
                            final_j["message"] = serde_json::json!({
                                "role": "assistant",
                                "content": "",
                                "tool_calls": tc
                            });
                        }
                        // 완료 패킷 출력 전송
                        yield Ok::<_, anyhow::Error>(format!("{}\n", final_j.to_string()));
                    }
                }
            }
        };
        
        // 조립된 NDJSON 비동기 스트림을 Axum 전용 HTTP Response 스트림 컨테이너에 매칭하여 회신합니다.
        Response::builder()
            // 응답 포맷을 x-ndjson 타입으로 명시합니다.
            .header("Content-Type", "application/x-ndjson")
            // 스트림 바디 설정
            .body(Body::from_stream(stream))
            // 에러 없이 완성
            .unwrap()
    } else {
        // 비스트리밍(한 번에 전체를 취합 응답)을 요청했을 때의 처리 분기입니다.
        // 역대 모인 답변 글자들을 모아줄 빈 문자열 버퍼입니다.
        let mut final_text = String::new();
        // 최종으로 보낼 대화 목록을 저장해둘 리스트입니다.
        let mut last_history: Vec<serde_json::Value> = Vec::new();
        // 최종 보고용 도구 사양서 임시 가드입니다.
        let mut last_tool_calls: Option<serde_json::Value> = None;
        
        // 에이전트 채널의 진행 완료 알림이 뜰 때까지 순회를 돌며 토큰 정보들을 계속 조용히 누적 축적합니다.
        while let Some(event) = event_rx.recv().await {
            // 이벤트 점검
            match event {
                // 생성 텍스트 적재
                ServerStreamEvent::Chunk(c) => {
                    // 합산 문자열 가산
                    final_text.push_str(&c);
                }
                // 도구 가동 명령은 마지막 통보를 위해 기억 처리
                ServerStreamEvent::ToolCall { raw_tool_calls_json } => {
                    // JSON 해석 시도
                    if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_tool_calls_json) {
                        // tool_calls 키 색출 적용
                        if let Some(tc) = j.get("tool_calls") {
                            // 임시 가드에 할당
                            last_tool_calls = Some(tc.clone());
                        }
                    }
                }
                // 비스트리밍이므로 중간 도구 실행이나 안내 가이드는 따로 클라이언트에 보내지 않고 무시합니다.
                ServerStreamEvent::ToolResult { .. } => {}
                ServerStreamEvent::Guidance(..) => {}
                // 연산 실패 시 즉시 INTERNAL_SERVER_ERROR(500) 코드로 비상 탈출 보고를 돌립니다.
                ServerStreamEvent::Error(e) => {
                    // 500 에러 전송
                    return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
                }
                // 완료 신호를 만나면 누적 집계된 대화 목록을 대입하고 취합 단계를 끝냅니다.
                ServerStreamEvent::Done { final_history, .. } => {
                    // 역사 갱신
                    last_history = final_history;
                }
            }
        }
        
        // Ollama 규약의 비스트림 최종 완성 JSON 아웃풋 구조를 만듭니다.
        let mut res_j = serde_json::json!({
            "model": state.model_name,
            "message": {
                "role": "assistant",
                "content": final_text
            },
            "done": true,
            "history": last_history
        });
        // 도구 가동 기록을 유실 없이 확보해 두었다가 결과 팩에 보강 입력해줍니다.
        if let Some(tc) = last_tool_calls {
            // 도구 호출 패킷 적용
            res_j["message"] = serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": tc
            });
        }
        // 최종 조립 완성된 결과 패킷을 JSON으로 인가 반환합니다.
        Json(res_j).into_response()
    }
}

// OpenAI 호환성 대화 completions 경로("/v1/chat/completions" 또는 "/chat/completions")에 매핑되는 엔드포인트입니다.
pub async fn handle_completions(
    // 서버 전역 공유 상태를 가져옵니다.
    State(state): State<AppState>,
    // 본문 원시 요청 텍스트를 수령합니다.
    req_body: String,
) -> impl IntoResponse {
    // 요청 패킷 형식을 ChatRequest 구조로 해석하며 문법 불능 시 400 에러 처리합니다.
    let req: ChatRequest = match serde_json::from_str(&req_body) {
        // 성공 시 이관
        Ok(r) => r,
        // 실패 시 400 반환
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    
    // 스트리밍 플래그를 가져옵니다.
    let want_stream = req.stream.unwrap_or(false);
    
    // 시스템 성격 지침 초기설정을 카피합니다.
    let mut sys_msg = state.system_prompt.clone();
    // 대화 히스토리 리스트 구조를 초기화합니다.
    let mut history_arr = serde_json::json!([]);
    // 이번 턴의 신규 유저 발문 구조를 구성합니다.
    let mut current_msg_j = serde_json::json!({
        "role": "user",
        "content": ""
    });
    
    // 메시지 묶음이 수신되었을 경우 역대 전체 라운드 대화 내역 분해 처리를 전개합니다.
    if let Some(messages) = &req.messages {
        // 메시지 내부 요소 검사
        if !messages.is_empty() {
            // 가장 마지막 질문은 이번 턴 발문이므로 수거합니다.
            current_msg_j = messages.last().unwrap().clone();
            // 전체 배열 수를 도출합니다.
            let len = messages.len();
            // 마지막 라운드를 제외한 이전 대화 기록을 돌며 보존 역사 목록에 차례로 등기합니다.
            for i in 0..len - 1 {
                // 특정 지점 메시지
                let msg = &messages[i];
                // 역할이 system이면 성격 선언을 교체 갱신합니다.
                if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
                    // 콘텐츠 갱신
                    if let Some(c) = msg.get("content").and_then(|c| c.as_str()) {
                        // 치환 완료
                        sys_msg = c.to_string();
                    }
                } else {
                    // 일반 대화 내역은 역사 목록에 머지합니다.
                    history_arr.as_array_mut().unwrap().push(msg.clone());
                }
            }
        }
    }
    
    // 모델 튜닝 파라미터를 규격에 부합하도록 포맷팅합니다.
    let mut opt = serde_json::json!({
        "max_output_tokens": 262144,
        "temperature": 0.7,
        "top_p": 0.95,
        "top_k": 40
    });
    // 옵션 세부 맵이 인가되어 들어온 경우 오버라이딩을 진행합니다.
    if let Some(ref req_opt) = req.options {
        // 온도 세팅
        if let Some(t) = req_opt.get("temperature") { opt["temperature"] = t.clone(); }
        // top_p 세팅
        if let Some(p) = req_opt.get("top_p") { opt["top_p"] = p.clone(); }
        // top_k 세팅
        if let Some(k) = req_opt.get("top_k") { opt["top_k"] = k.clone(); }
        // 토큰 상한 세팅
        if let Some(m) = req_opt.get("max_output_tokens") { opt["max_output_tokens"] = m.clone(); }
        // 대체 토큰 필드 적용
        if let Some(m) = req_opt.get("max_tokens") { opt["max_output_tokens"] = m.clone(); }
        // 대체 예측 필드 적용
        if let Some(m) = req_opt.get("num_predict") { opt["max_output_tokens"] = m.clone(); }
    } else {
        // 단독 상위 변수로 전송되었을 시의 동기화 처리입니다.
        if let Some(t) = req.temperature { opt["temperature"] = serde_json::json!(t); }
        // top_p 세팅
        if let Some(p) = req.top_p { opt["top_p"] = serde_json::json!(p); }
        // top_k 세팅
        if let Some(k) = req.top_k { opt["top_k"] = serde_json::json!(k); }
        // max_tokens 세팅
        if let Some(m) = req.max_tokens { opt["max_output_tokens"] = serde_json::json!(m); }
        // max_output_tokens 세팅
        if let Some(m) = req.max_output_tokens { opt["max_output_tokens"] = serde_json::json!(m); }
        // num_predict 세팅
        if let Some(m) = req.num_predict { opt["max_output_tokens"] = serde_json::json!(m); }
    }
    
    // 각 데이터를 에이전트 스레드 인계용 평문으로 직렬화 변환합니다.
    let history_json_str = history_arr.to_string();
    // 현재 메시지
    let current_msg_str = current_msg_j.to_string();
    // 하이퍼파라미터 사양
    let config_json_str = opt.to_string();
    
    // 엔진 참조 복제를 취득합니다.
    let engine = state.engine.clone();
    
    // 에이전트와 비동기로 정보교환할 통지 파이프 채널을 마련합니다.
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ServerStreamEvent>();
    
    // 비동기 스레드를 독립 기동하여 내부 에이전트 실행 루프를 격발시킵니다.
    std::thread::spawn(move || {
        // 에이전트 핵심 루프 실행
        run_agentic_loop(
            engine,
            sys_msg,
            history_json_str,
            current_msg_str,
            Some(config_json_str),
            event_tx,
        );
    });
    
    // OpenAI 규격 스트리밍을 요청했을 시의 리턴 분기입니다.
    if want_stream {
        // 내부 보관 중인 모델 이름 복제
        let model_name = state.model_name.clone();
        
        // async-stream 제너레이터 매크로를 기동하여 OpenAI 스트리밍 응답 본체를 조립합니다.
        let stream = async_stream::stream! {
            // 중간 기록용 도구 호출 임시 보관 가드입니다.
            let mut last_tool_calls: Option<serde_json::Value> = None;
            // 통지 소켓 수신기에서 정보 조각을 꺼내 순회합니다.
            while let Some(event) = event_rx.recv().await {
                // 이벤트 타겟팅 매칭
                match event {
                    // 실시간 답변 조각 텍스트 획득 상황
                    ServerStreamEvent::Chunk(c) => {
                        // OpenAI 규격의 스트림 조각 JSON(chat.completion.chunk)을 구성합니다.
                        let chunk_j = serde_json::json!({
                            "id": "chatcmpl-litert",
                            "object": "chat.completion.chunk",
                            "created": chrono::Utc::now().timestamp(),
                            "model": model_name,
                            "choices": [
                                {
                                    // 델타 필드 아래에 글자 단편을 실어 보냅니다.
                                    "delta": {
                                        "content": c
                                    },
                                    // 턴 진행 중이므로 마감 사유는 null로 세팅합니다.
                                    "finish_reason": serde_json::Value::Null
                                }
                            ]
                        });
                        // SSE 프로토콜 규격 양식("data: JSON\n\n")에 맞춘 평문 포맷을 수집 방출합니다.
                        yield Ok::<_, anyhow::Error>(format!("data: {}\n\n", chunk_j.to_string()));
                    }
                    // 모델이 도구를 사용할 것임을 보고받았을 때의 처리입니다.
                    ServerStreamEvent::ToolCall { raw_tool_calls_json } => {
                        // 도구 지시 데이터 파싱 검증
                        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_tool_calls_json) {
                            // 내부 tool_calls 배열이 존재할 때 임시 가드에 동기화 백업합니다.
                            if let Some(tc) = j.get("tool_calls") {
                                // 임시 보관 가드 갱신
                                last_tool_calls = Some(tc.clone());
                            }
                        }
                    }
                    // 비스트리밍 요소이므로 도구 실행 및 시스템 가이드는 중계하지 않고 무시 처리합니다.
                    ServerStreamEvent::ToolResult { .. } => {}
                    ServerStreamEvent::Guidance(..) => {}
                    // 에러 검출 시 즉각 비상 탈출을 위해 예외를 상위 스트림으로 방출하고 종료합니다.
                    ServerStreamEvent::Error(e) => {
                        // 예외 방출
                        yield Err(anyhow::anyhow!(e));
                    }
                    // 에이전트 루프가 Done 완료 신호를 도출해낸 최종 피날레 턴의 응답 가공입니다.
                    ServerStreamEvent::Done { .. } => {
                        // 기억해 둔 도구 호출 가드 정보가 있다면 해당 내역을 동봉하여 종료 선언을 격발합니다.
                        if let Some(tc) = last_tool_calls.take() {
                            // 도구 사용 알림용 OpenAI 스트림 응답 프레임 빌드
                            let chunk_j = serde_json::json!({
                                "id": "chatcmpl-litert",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": model_name,
                                "choices": [
                                    {
                                        // delta 아래에 도구 지시 구조 탑재
                                        "delta": {
                                            "tool_calls": tc
                                        },
                                        // 마감 완료 이유를 tool_calls로 명시 기입
                                        "finish_reason": "tool_calls"
                                    }
                                ]
                            });
                            // yield 전송
                            yield Ok::<_, anyhow::Error>(format!("data: {}\n\n", chunk_j.to_string()));
                        } else {
                            // 단순 텍스트 답변이 자연스럽게 완성되어 종결되었을 때의 처리입니다.
                            let chunk_j = serde_json::json!({
                                "id": "chatcmpl-litert",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": model_name,
                                "choices": [
                                    {
                                        // 빈 델타
                                        "delta": {},
                                        // 자연스러운 종료(stop) 명시
                                        "finish_reason": "stop"
                                    }
                                ]
                            });
                            // yield 전송
                            yield Ok::<_, anyhow::Error>(format!("data: {}\n\n", chunk_j.to_string()));
                        }
                        // OpenAI SSE 규격에 준한 스트림 완전 종료 안내 표식(data: [DONE])을 발송 완수합니다.
                        yield Ok::<_, anyhow::Error>("data: [DONE]\n\n".to_string());
                    }
                }
            }
        };
        
        // 최종 조립 완료된 SSE 규약 스트림 본체를 HTTP Response 객체로 포장해 클라이언트로 쏴줍니다.
        Response::builder()
            // 응답 헤더 콘텐츠 형식을 text/event-stream 규격으로 명시 선언합니다.
            .header("Content-Type", "text/event-stream")
            // 스트림 바디 설정
            .body(Body::from_stream(stream))
            // 에러 없이 완성
            .unwrap()
    } else {
        // OpenAI 비스트리밍(한 번에 전체 묶음 패킷 응답) 방식을 요청했을 때의 처리 분기입니다.
        // 역대 모인 답변 글자들을 담아둘 빈 문자열 버퍼입니다.
        let mut final_text = String::new();
        // 최후 보고용 도구 사양서 임시 가드입니다.
        let mut last_tool_calls: Option<serde_json::Value> = None;
        
        // 에이전트 채널의 진행 완료 알림이 뜰 때까지 순회를 돌며 토큰 정보들을 계속 조용히 누적 축적합니다.
        while let Some(event) = event_rx.recv().await {
            // 이벤트 점검
            match event {
                // 생성 텍스트 적재
                ServerStreamEvent::Chunk(c) => {
                    // 합산 문자열 가산
                    final_text.push_str(&c);
                }
                // 도구 가동 명령은 마지막 통보를 위해 기억 처리
                ServerStreamEvent::ToolCall { raw_tool_calls_json } => {
                    // JSON 해석 시도
                    if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_tool_calls_json) {
                        // tool_calls 키 색출 적용
                        if let Some(tc) = j.get("tool_calls") {
                            // 임시 가드에 할당
                            last_tool_calls = Some(tc.clone());
                        }
                    }
                }
                // 중간 도구 결과나 가이드는 중계 응답 대상이 아니므로 무시합니다.
                ServerStreamEvent::ToolResult { .. } => {}
                ServerStreamEvent::Guidance(..) => {}
                // 연산 실패 발생 시 내부 서버 오류(500) 상태 코드를 실어 곧바로 에러 사유를 회신합니다.
                ServerStreamEvent::Error(e) => {
                    // 500 에러 전송
                    return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
                }
                // 완료 신호를 받으면 취합을 마칩니다.
                ServerStreamEvent::Done { .. } => {}
            }
        }
        
        // OpenAI 비스트리밍 completions 리턴 포맷에 적합한 초안 초이스 데이터셋 구조를 마련합니다.
        let mut choice = serde_json::json!({
            "message": {
                "role": "assistant",
                "content": final_text
            },
            // 일반 자연 완성 stop 기재
            "finish_reason": "stop"
        });
        
        // 만약 도구 가동 내역이 확보되었던 경우, 초이스 모델 데이터를 도구 규격(tool_calls)으로 갱신 재정돈해 줍니다.
        if let Some(tc) = last_tool_calls {
            // 도구 결과 패킷 오버라이딩
            choice = serde_json::json!({
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": tc
                },
                // 종료 이유를 도구 기동으로 명시
                "finish_reason": "tool_calls"
            });
        }
        
        // OpenAI 규정 chat.completion 최종 명세 프레임을 조립 구축합니다.
        let res_j = serde_json::json!({
            "id": "chatcmpl-litert",
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": state.model_name,
            // 조립 완료된 초이스 삽입
            "choices": vec![choice]
        });
        // JSON 응답으로 응수
        Json(res_j).into_response()
    }
}
