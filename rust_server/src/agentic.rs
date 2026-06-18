// C 라이브러리 원시 바인딩을 sys 명칭으로 가져옵니다.
use litert_lm_sys as sys;
// ffi 모듈에서 FFI 함수 및 EngineWrapper 설정을 참조하기 위해 임포트합니다.
use crate::ffi::{
    litert_lm_conversation_get_benchmark_info, litert_lm_conversation_send_message_stream,
    litert_lm_benchmark_info_delete, litert_lm_benchmark_info_get_decode_token_count_at,
    litert_lm_benchmark_info_get_num_decode_turns, litert_lm_benchmark_info_get_num_prefill_turns,
    litert_lm_benchmark_info_get_prefill_token_count_at, litert_lm_conversation_optional_args_create,
    litert_lm_conversation_optional_args_delete, litert_lm_conversation_optional_args_set_max_output_tokens,
    litert_lm_conversation_optional_args_set_visual_token_budget, EngineWrapper,
};
// utils 모듈에서 프롬프트 로딩, JSON 정제, 매개변수 파싱용 편의 기능들을 가져옵니다.
use crate::utils::{
    extract_text_from_chunk, get_iso8601_now, load_merged_tools, parse_all_custom_tool_calls,
};
// tools 모듈에서 실제 AI의 기능 도구를 수행해 주는 실행 통로를 임포트합니다.
use crate::tools::execute_tool;

// C언어 문자열 컨버팅 및 원시 포인터 핸들링을 위해 표준 패키지를 가져옵니다.
use std::ffi::{CStr, CString};
// 원시 Null 포인터를 다루기 위해 ptr 모듈을 가져옵니다.
use std::ptr;
// 여러 스레드 간 엔진 공유를 지원하도록 Arc 참조 카운팅 래퍼를 사용합니다.
use std::sync::Arc;
// 비동기 스레드 간 데이터 파이프 전송을 위해 Tokio의 mpsc 채널을 가져옵니다.
use tokio::sync::mpsc;
// 에러 처리를 위한 anyhow 도구를 가져옵니다.
use anyhow::anyhow;

// FFI 비동기 콜백 스레드로부터 메인 루프 측으로 스트리밍 단편 조각을 전송할 때 쓰이는 중간 메일함용 열거형입니다.
pub enum StreamMessage {
    // 모델이 부분 실시간 생성해 낸 한 조각의 텍스트 토큰입니다.
    Chunk(String),
    // FFI를 통해 C 문자열 바이트 배열 형태로 넘어온 가공되지 않은 순수 JSON 원시 텍스트 버퍼입니다.
    RawBuffer(String),
    // 생성이 정상 완수되었거나 내부 FFI 예외 오류로 인해 비정상 파괴 종료되었음을 전파하는 최종 플래그입니다.
    Final {
        // 오류로 끝났는지 여부
        has_error: bool,
        // 발생한 오류 원인 텍스트 메시지
        error_msg: Option<String>,
    },
}

// C API 엔진이 새 텍스트 토큰을 뿌릴 때마다 시스템에 의해 내부 격발되는 C 선언용 콜백 함수 본체입니다.
pub unsafe extern "C" fn stream_callback(
    // 메인 루프 측 채널 송신기를 전달받는 사용자 임의 컨텍스트 원시 포인터입니다.
    callback_data: *mut std::ffi::c_void,
    // 엔진이 출력한 신규 생성 토큰 텍스트의 캐릭터 포인터입니다.
    chunk: *const std::os::raw::c_char,
    // 이 턴의 생성이 완벽히 끝났음을 알려주는 종료 시그널 참거짓 값입니다.
    is_final: bool,
    // 오류가 났을 때 원인 문장이 담긴 에러 캐릭터 포인터입니다.
    error_msg: *const std::os::raw::c_char,
) {
    // callback_data 주소를 UnboundedSender 타입 주소로 역참조 형변환하여 획득합니다.
    let sender = &*(callback_data as *const mpsc::UnboundedSender<StreamMessage>);
    
    // 에러 포인터가 널이 아니라면 내부 심각한 추론 장애가 터진 상태를 전파하고 조기 퇴근합니다.
    if !error_msg.is_null() {
        // C 문자열을 Rust의 소유권 있는 String 객체로 해독 추출합니다.
        let err_str = CStr::from_ptr(error_msg).to_string_lossy().into_owned();
        // 채널을 통해 오류 완료 선언을 메인 스레드에 보냅니다.
        let _ = sender.send(StreamMessage::Final {
            has_error: true,
            error_msg: Some(err_str),
        });
        return;
    }
    
    // 청크 데이터 포인터가 널이 아닐 경우 신규 토큰이 확보된 것이므로 텍스트 가공을 돌립니다.
    if !chunk.is_null() {
        // C 문자열 캐릭터 버퍼를 안전하게 Rust String 객체로 형변환합니다.
        let raw_chunk = CStr::from_ptr(chunk).to_string_lossy().into_owned();
        // 디버그용 콘솔창에 수신된 원시 청크 단편 값을 모니터링 출력합니다.
        println!("[DEBUG CHUNK] {}", raw_chunk);
        // 청크 구조 분석기를 기동하여 클라이언트 응답용 가공 텍스트만 빼냅니다.
        let extracted = extract_text_from_chunk(&raw_chunk);
        // 걸러낸 텍스트가 빈칸이 아니면 실시간 텍스트 조각 채널 전송을 쏩니다.
        if !extracted.is_empty() {
            // Chunk 메시지 격발
            let _ = sender.send(StreamMessage::Chunk(extracted));
        }
        // 원시 로깅 분석을 위해 필터링하지 않은 날 것 그대로의 원시 청크 텍스트도 전달합니다.
        let _ = sender.send(StreamMessage::RawBuffer(raw_chunk));
    }
    
    // 생성 사이클이 끝을 만났다면 안전하게 종료 신호를 송출합니다.
    if is_final {
        // Final 완료 선언 전파
        let _ = sender.send(StreamMessage::Final {
            has_error: false,
            error_msg: None,
        });
    }
}

// C 라이브러리의 벤치마크 누적 데이터를 추적하여 프리필 단계와 디코드 생성 단계의 누적 토큰 계수를 얻어오는 함수입니다.
pub fn get_conversation_token_counts(conversation: *mut sys::LiteRtLmConversation) -> (i32, i32) {
    // 전달받은 대화 메모리 구조 포인터가 널이면 집계가 불가능하므로 0을 반환합니다.
    if conversation.is_null() {
        return (0, 0);
    }
    // C API 다이렉트 핸들링이 수반되므로 안전성 감시를 위해 unsafe 블록을 전개합니다.
    unsafe {
        // 대화 객체로부터 물리 벤치마크 추적 구조체 포인터를 발굴해 냅니다.
        let info = litert_lm_conversation_get_benchmark_info(conversation);
        // 벤치마크 수집 결과 주소가 널인 경우 수집 실패로 판단하고 기본값을 반환합니다.
        if info.is_null() {
            return (0, 0);
        }
        // 내부 정보판을 스캔하여 프리필(질문 접수) 턴의 전체 회수를 계산합니다.
        let num_prefill = litert_lm_benchmark_info_get_num_prefill_turns(info);
        // 분석 정보판을 스캔하여 디코드(답변 생성) 턴의 전체 회수를 계산합니다.
        let num_decode = litert_lm_benchmark_info_get_num_decode_turns(info);
        
        // 프리필 총 소비 토큰 수 합산기입니다.
        let mut prefill_tokens = 0;
        // 각 프리필 회차를 돌며 누적 토큰을 가산합니다.
        for i in 0..num_prefill {
            // 인덱스 i 지점의 프리필 토큰을 가져와 누적 합산합니다.
            prefill_tokens += litert_lm_benchmark_info_get_prefill_token_count_at(info, i);
        }
        
        // 디코드 총 생성 토큰 수 합산기입니다.
        let mut decode_tokens = 0;
        // 각 디코드 라운드를 돌며 토큰을 모아 더해줍니다.
        for i in 0..num_decode {
            // 인덱스 i 지점의 디코드 토큰 개수를 가져와 더해줍니다.
            decode_tokens += litert_lm_benchmark_info_get_decode_token_count_at(info, i);
        }
        
        // 사용이 완료된 임시 벤치마크 계측 객체의 메모리를 해제합니다.
        litert_lm_benchmark_info_delete(info);
        // 프리필 토큰 및 디코드 토큰 개수 조합 튜플을 반환합니다.
        (prefill_tokens as i32, decode_tokens as i32)
    }
}

// 클라이언트 사이드 스트리밍 연결로 중계해 줄 각종 내부 상태 변화 이벤트 전파용 열거형입니다.
pub enum ServerStreamEvent {
    // 텍스트 조각을 스트리밍하는 이벤트입니다.
    Chunk(String),
    // 모델이 도구를 기동하겠다는 JSON 호출부를 뱉었을 때 발송하는 알림 이벤트입니다.
    ToolCall {
        // 원시 도구 선언 JSON 문장
        raw_tool_calls_json: String,
    },
    // 기동된 도구의 파이썬 처리 최종 콘솔 표준출력 결과물을 획득했을 때 뿌리는 이벤트입니다.
    ToolResult {
        // 동작한 도구의 고유명칭
        name: String,
        // 도구 콘솔 리턴 결과물
        result: String,
    },
    // 현재 에이전트 루프가 어떠한 공정 지침(예: "도구를 만듭니다...")을 타는 중인지 사용자 UI에 알려주는 진행 안내문 이벤트입니다.
    Guidance(String),
    // 에이전트 루프의 모든 작업 체인이 완결되었을 때 수집 데이터와 역대 대화 역사를 담아 발송하는 최종 종결 이벤트입니다.
    Done {
        // 합산 수정이 이루어진 최종 전체 대화 리스트
        final_history: Vec<serde_json::Value>,
        // 질문 분석(프리필)에 들어간 총 토큰 예산
        prompt_tokens: i32,
        // 대답 생성(디코드)에 쓰인 총 토큰 소모량
        completion_tokens: i32,
    },
    // 전체 공정 진행 중 해소하기 힘든 에러가 터졌을 때 중단과 함께 쏘는 비상 오류 이벤트입니다.
    Error(String),
}

// 자율 동적 도구 루프(Agentic Loop)의 실행 엔진을 독립적으로 기동해주는 핵심 에이전트 메인 루프입니다.
pub fn run_agentic_loop(
    // 래핑되어 보호받는 LiteRT-LM 엔진 구조체에 대한 Arc 스레드 가드 레퍼런스입니다.
    engine: Arc<EngineWrapper>,
    // 현재 사용할 시스템 프롬프트(성격 + 도구 가이드 통합본) 문자열입니다.
    system_msg_str: String,
    // 직전까지 쌓인 이전 대화 턴 히스토리 원시 JSON 데이터입니다.
    history_json: String,
    // 이번 턴에 새로 접수된 사용자의 실시간 질의 본문 JSON 텍스트입니다.
    current_msg: String,
    // 온도(temperature) 등 하이퍼파라미터 모델 상세 튜닝용 JSON 옵션 설정 문자열입니다.
    config_json: Option<String>,
    // 생성된 스트림 이벤트를 비동기로 중계하고 전파하기 위한 송신기(Sender) 객체입니다.
    event_tx: mpsc::UnboundedSender<ServerStreamEvent>,
) {
    // 히스토리 원시 JSON을 Rust가 파싱하기 좋은 JSON 배열로 해석하고 없거나 깨진 구조면 빈 구조를 인스턴스화합니다.
    let mut local_history: serde_json::Value = if !history_json.is_empty() {
        serde_json::from_str(&history_json).unwrap_or(serde_json::json!([]))
    } else {
        serde_json::json!([])
    };
    // 안전장치로 로컬 히스토리 데이터 최상위 구조가 배열 포맷인지 한 번 더 감시하고 강제 보정합니다.
    if !local_history.is_array() {
        // 빈 배열 생성
        local_history = serde_json::json!([]);
    }
    
    // 에이전트가 여러 도구를 생성/기동하는 전체 과정 동안 소모한 총 질문 분석 토큰 카운터입니다.
    let mut total_prompt_tokens = 0i32;
    // 모든 체인 단계 동안 모델이 글자를 찍어낼 때 소모한 총 대답 토큰 카운터입니다.
    let mut total_completion_tokens = 0i32;
    
    // 계속 꼬리를 물며 업데이트되는 현재 차례의 입력 메시지 변수로, 사용자 질의로 시작 기점을 잡습니다.
    let mut active_msg = current_msg.clone();
    
    // 1단계: 대화 역사 목록 내부에 예전에 보냈던 낡고 중복된 성격 지침(system) 내용들이 남아있다면 정제하여 날려버립니다.
    if let Some(arr) = local_history.as_array_mut() {
        // 역할군이 "system"으로 명명된 객체들만 제거하여 대화 목록의 팽창을 막습니다.
        arr.retain(|msg| msg.get("role").and_then(|r| r.as_str()) != Some("system"));
    }

    // 2단계: 첫 번째 질문을 던지는 사용자 턴에 최종 시스템 프롬프트를 융합(prepend)시켜 모델의 도구 실행 규격 밀착도가 최고조를 이루도록 돕습니다.
    let is_history_empty = local_history.as_array().map_or(true, |a| a.is_empty());
    // 히스토리가 비어있는 신규 채팅 대화 세션인 경우
    if is_history_empty {
        // 전송할 활성 메시지의 JSON 데이터 객체 구조 파싱을 전개합니다.
        if let Ok(mut msg_j) = serde_json::from_str::<serde_json::Value>(&active_msg) {
            // 본체 content 필드를 타깃으로 조정을 시작합니다.
            if let Some(content) = msg_j.get_mut("content") {
                // 본문 콘텐츠가 단순 스트링일 때
                if let Some(orig_content) = content.as_str() {
                    // 시스템 프롬프트가 이미 삽입되어 있는 중복 상태인지 확인합니다.
                    if !orig_content.contains(&system_msg_str) {
                        // 맨 앞에 시스템 메시지를 집어넣고 뒤에 유저 원문 본문을 연동한 형태로 콘텐츠를 갱신합니다.
                        *content = serde_json::json!(format!("{}\n\n{}", system_msg_str, orig_content));
                        // 전송 메시지 데이터에 수정 사항을 주입합니다.
                        active_msg = msg_j.to_string();
                    }
                } else if let Some(arr) = content.as_array_mut() {
                    // 멀티모달 포맷 배열 형식으로 메시지 본문이 세팅된 경우
                    let mut already_contains = false;
                    // 내부 요소 중 중복 삽입 여부를 체크합니다.
                    for item in arr.iter() {
                        // 타입이 text 포맷인 원소를 골라냅니다.
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            // 텍스트 내용 안에 시스템 지침서가 이미 머지되어 있는지 검출합니다.
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                // 존재 시 마킹
                                if text.contains(&system_msg_str) {
                                    already_contains = true;
                                    break;
                                }
                            }
                        }
                    }
                    // 이미 머지된 기록이 존재하지 않는 신규 대상일 때
                    if !already_contains {
                        // 배열 인자 속에서 실제 텍스트 원소를 찾아 가장 전방에 성격 프롬프트를 머지해 줍니다.
                        for item in arr {
                            // 텍스트 블록
                            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                                // 콘텐츠 텍스트 객체 확보
                                if let Some(text) = item.get_mut("text") {
                                    // 기존 텍스트를 취득
                                    if let Some(orig_text) = text.as_str() {
                                        // 성격 지침 융합 처리
                                        *text = serde_json::json!(format!("{}\n\n{}", system_msg_str, orig_text));
                                        break;
                                    }
                                }
                            }
                        }
                        // 변경 데이터를 문자열로 직렬화하여 세팅합니다.
                        active_msg = msg_j.to_string();
                    }
                }
            }
        }
    } else {
        // 이미 지나간 이전 대화 기록이 들어있는 상태인 경우 대화 역사의 가장 최초 첫 턴 메일 내용에 지침서를 융합시킵니다.
        if let Some(first_msg) = local_history.as_array_mut().and_then(|a| a.first_mut()) {
            // 첫 턴의 콘텐츠 영역을 타깃 삼습니다.
            if let Some(content) = first_msg.get_mut("content") {
                // 단일 문자열 포맷 본문일 경우
                if let Some(orig_content) = content.as_str() {
                    // 중복 여부를 감지
                    if !orig_content.contains(&system_msg_str) {
                        // 성격 선언 추가 삽입
                        *content = serde_json::json!(format!("{}\n\n{}", system_msg_str, orig_content));
                    }
                } else if let Some(arr) = content.as_array_mut() {
                    // 첫 메일이 멀티모달 배열 형식인 경우
                    let mut already_contains = false;
                    // 내용 스캔
                    for item in arr.iter() {
                        // 텍스트 타깃 확인
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            // 성격 문구가 이미 들어있는지 조사
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                // 존재 시 마킹
                                if text.contains(&system_msg_str) {
                                    already_contains = true;
                                    break;
                                }
                            }
                        }
                    }
                    // 포함되지 않았을 때
                    if !already_contains {
                        // 텍스트 항목을 추출해 냅니다.
                        for item in arr {
                            // 텍스트 블록
                            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                                // 내부 데이터 스캔
                                if let Some(text) = item.get_mut("text") {
                                    // 원본 확보
                                    if let Some(orig_text) = text.as_str() {
                                        // 성격 내용 융합
                                        *text = serde_json::json!(format!("{}\n\n{}", system_msg_str, orig_text));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 3단계: 이제 시스템 지침서 단독 프롬프트 오브젝트를 대화 내역 전체 배열의 가장 첫 인덱스(0번)에 최종 투하합니다.
    if let Some(arr) = local_history.as_array_mut() {
        // 인덱스 0번에 주입
        arr.insert(0, serde_json::json!({
            "role": "system",
            "content": system_msg_str
        }));
    }
    
    // 디버그 콘솔창에 주입 완료된 시스템 프롬프트 글자 수 길이를 출력합니다.
    println!("[DEBUG] run_agentic_loop: system_msg_str length = {}", system_msg_str.len());
    // 현재 차례의 로컬 대화 히스토리 상태를 출력합니다.
    println!("[DEBUG] run_agentic_loop: local_history = {}", local_history);
    // 이번 턴의 핵심 활성 메시지를 표기합니다.
    println!("[DEBUG] run_agentic_loop: active_msg = {}", active_msg);
    
    // 루프가 과도하게 무한 뺑뺑이를 돌며 오동작하는 것을 구원하고자 최대 도구 체이닝 실행 한계를 10회로 픽스합니다.
    let mut loop_count = 0;
    // 10회 상한 조건 아래 루프를 격발합니다.
    while loop_count < 10 {
        // 반복 라운드 수치를 증가시킵니다.
        loop_count += 1;
        
        // 로컬 대화 히스토리가 비었는지 검사하여 API 전달용 Option 변수로 정리합니다.
        let history_str = if local_history.as_array().map_or(true, |a| a.is_empty()) {
            // 없으면 무효 처리
            None
        } else {
            // 존재하면 JSON 직렬화 문자열 형태로 탑재
            Some(local_history.to_string())
        };
        
        // 정적 + 동적 도구들을 모두 머지한 통합 도구 명세 json 문서를 가져옵니다.
        let tools_str = load_merged_tools();
        // 도구 목록이 단순히 빈 배열([])이라면 설정을 생략하고 그렇지 않으면 Option으로 포장합니다.
        let tools_opt = if tools_str == "[]" { None } else { Some(tools_str.as_str()) };
        
        // 비동기 스레드로부터 데이터 피드백을 전달받을 단방향 Unbounded MPSC 통신 채널을 오픈합니다.
        let (tx, mut rx) = mpsc::unbounded_channel::<StreamMessage>();
        
        // FFI를 통해 C라이브러리로 보낼 원시 포인터 주소 등을 준비합니다.
        let engine_ptr = engine.ptr.as_ptr();
        // 시스템 텍스트 복제
        let sys_prompt = system_msg_str.clone();
        // 전송 내용물 복사
        let active_msg_clone = active_msg.clone();
        // 튜닝용 설정 옵션 복제
        let config_json_clone = config_json.clone();
        
        // FFI 함수들을 활용해 대화 대리 객체(LiteRtLmConversation)를 생성하고 실제 스트림 전송을 쏘는 안전하지 않은 영역입니다.
        let run_res = unsafe {
            // 대화 세션에 사용될 세부 파라미터 구조체를 C API를 통해 생성합니다.
            let session_config = sys::litert_lm_session_config_create();
            // 기본 출력 리미트 예산을 262144로 초기화합니다.
            let mut max_output_tokens = 262144;
            // 튜닝 옵션이 들어왔을 때 각 값(온도, 최고 확률 후보 등)을 세션 설정 구조체에 전입시킵니다.
            if let Some(ref cfg_str) = config_json_clone {
                // 옵션 텍스트 파싱 진행
                if let Ok(cfg_j) = serde_json::from_str::<serde_json::Value>(cfg_str) {
                    // 최대 토큰 크기 변경 요청이 들어온 경우 반영
                    if let Some(max_tokens) = cfg_j.get("max_output_tokens").and_then(|v| v.as_i64()) {
                        // 세션 설정 구조체에 적용
                        sys::litert_lm_session_config_set_max_output_tokens(session_config, max_tokens as i32);
                        // 지역 제어 변수에도 동기화
                        max_output_tokens = max_tokens as i32;
                    }
                    // 온도(temperature) 값을 읽어오고 기본값은 0.7로 처리합니다.
                    let temp = cfg_j.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32;
                    // 누적 확률 한계치(top_p)를 읽어오고 기본값은 0.95로 처리합니다.
                    let top_p = cfg_j.get("top_p").and_then(|v| v.as_f64()).unwrap_or(0.95) as f32;
                    // 상위 후보군 범위(top_k)를 읽어오고 기본값은 40으로 처리합니다.
                    let top_k = cfg_j.get("top_k").and_then(|v| v.as_i64()).unwrap_or(40) as i32;
                    
                    // 온도계수가 0 이하(그리디 서치)인지 조건 비교하여 알맞은 C 샘플러 분류 열거형을 책정합니다.
                    let sampler_type = if temp <= 0.0 {
                        // Greedy 모드 선택
                        sys::kGreedy
                    } else if top_p < 1.0 {
                        // 확률 누적 TopP 모드 선택
                        sys::kTopP
                    } else {
                        // 개수 제한 TopK 모드 선택
                        sys::kTopK
                    };
                    // C 규격의 샘플링 수치 파라미터 구조체에 조립 대입합니다.
                    let sampler_params = sys::LiteRtLmSamplerParams {
                        type_: sampler_type,
                        top_k,
                        top_p,
                        temperature: temp,
                        seed: 0,
                    };
                    // 세션 옵션 구조체에 샘플러 데이터 묶음을 인가합니다.
                    sys::litert_lm_session_config_set_sampler_params(session_config, &sampler_params);
                }
            }
            
            // 시스템 성격 지침 데이터를 C 규격의 JSON 프레임워크 텍스트 구조로 직렬화 빌드합니다.
            let sys_json = serde_json::json!({
                "role": "system",
                "content": sys_prompt
            }).to_string();
            
            // C언어 함수 인자로 전달 가능한 널 종료 CString으로 객체 변환합니다.
            let sys_cstr = CString::new(sys_json).unwrap();
            // 머지한 활성 도구 명세도 C 문자열 변환 처리합니다.
            let tools_cstr = tools_opt.map(|s| CString::new(s).unwrap());
            // 누적 히스토리도 마찬가지로 C 문자열로 포맷팅합니다.
            let history_cstr = history_str.as_ref().map(|s| CString::new(s.as_str()).unwrap());
            
            // C API를 동원해 대화 설정용 구성 명세서(conv_config) 객체를 구축합니다.
            let conv_config = sys::litert_lm_conversation_config_create(
                engine_ptr,
                session_config,
                sys_cstr.as_ptr(),
                tools_cstr.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                history_cstr.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                // constrained decoding 기능은 무효화합니다.
                false,
            );
            // 설정 인가가 종결되었으므로 임시로 쓰인 세션 설정은 메모리 정리합니다.
            sys::litert_lm_session_config_delete(session_config);
            
            // 구성 명세서 생성이 취소되었을 시 에러를 반환합니다.
            if conv_config.is_null() {
                // anyhow 에러 반환
                Err(anyhow!("Failed to create conv config"))
            } else {
                // 구성 사양서를 토대로 실제 가동용 대화 조율 인스턴스(conversation)를 띄웁니다.
                let conversation = sys::litert_lm_conversation_create(engine_ptr, conv_config);
                // 인스턴스를 빌드했으므로 임시 사양서 객체는 메모리 회수합니다.
                sys::litert_lm_conversation_config_delete(conv_config);
                // 대화 인스턴스 획득에 차질이 생겼을 때의 예외 처리입니다.
                if conversation.is_null() {
                    // 에러 반환
                    Err(anyhow!("Failed to create conversation"))
                } else {
                    // 활성 메시지 텍스트를 CString 구조로 감싸 줍니다.
                    let active_msg_cstr = CString::new(active_msg_clone).unwrap();
                    // 비동기 콜백 스레드가 참조해 데이터를 쏴줄 송신기(Sender) 객체를 힙 메모리에 완전히 고정 적재(Box::into_raw)시킵니다.
                    let tx_ptr = Box::into_raw(Box::new(tx));
                    
                    // 부가적인 옵션 파라미터를 추가 조율하기 위한 임시 옵션 지시판을 생성합니다.
                    let opt_args = litert_lm_conversation_optional_args_create();
                    // 지시판 로드 성공 시 상세 규격을 적용합니다.
                    if !opt_args.is_null() {
                        // 이미지 입력 시 처리할 최대 시각 토큰 한계를 설정합니다.
                        litert_lm_conversation_optional_args_set_visual_token_budget(opt_args, 1024);
                        // 답변 최대 크기 옵션을 반영합니다.
                        litert_lm_conversation_optional_args_set_max_output_tokens(opt_args, max_output_tokens);
                    }
                    
                    // C API 비동기 스트림 발송기를 쏘아 추론 연산 처리를 실시간으로 실행 가동합니다.
                    let ret = litert_lm_conversation_send_message_stream(
                        conversation,
                        active_msg_cstr.as_ptr(),
                        ptr::null(),
                        opt_args,
                        // 정의해둔 콜백 함수 포인터를 연결합니다.
                        Some(stream_callback),
                        // 힙 영역에 박제해 둔 송신기 포인터를 데이터 참조 컨텍스트로 함께 넘깁니다.
                        tx_ptr as *mut std::ffi::c_void,
                    );
                    
                    // 옵션 보조판에 배당된 메모리를 정리합니다.
                    if !opt_args.is_null() {
                        // 메모리 해제
                        litert_lm_conversation_optional_args_delete(opt_args);
                    }
                    
                    // 대화 포인터 주소, 송신기 원시 주소, API 동작 결과 성공 코드를 동봉해 리턴합니다.
                    Ok((conversation, tx_ptr, ret))
                }
            }
        };
        
        // FFI 구동 트라이 최종 검증 결과를 안전지대(Safe Rust)에서 배부받아 세팅합니다.
        let (conversation, tx_ptr, ret) = match run_res {
            // 성공 시 결과 인자들을 인계받음
            Ok(val) => val,
            // 예외 발생 시 에러 알림 채널 송출 후 루프 완전 파괴 종료
            Err(e) => {
                // 에러 알림
                let _ = event_tx.send(ServerStreamEvent::Error(e.to_string()));
                break;
            }
        };
        
        // API 리턴 코드가 0이 아니라면 기동 실패 상황이므로 자원을 회수하고 종결 처리합니다.
        if ret != 0 {
            // 에러 전달
            let _ = event_tx.send(ServerStreamEvent::Error(format!("Stream start failed: {}", ret)));
            // 안전 확보 후 원시 데이터 파괴
            unsafe {
                // 대화 자원 소멸
                sys::litert_lm_conversation_delete(conversation);
                // 박제된 송신기를 다시 Rust의 똑똑한 가비지 컬렉터(Box)로 회수하여 스코프를 통해 소멸시킵니다.
                let _ = Box::from_raw(tx_ptr);
            }
            break;
        }
        
        // 수신받아 가공한 텍스트 데이터 총합을 기억해두는 로컬 변수입니다.
        let mut full_response_content = String::new();
        // FFI에서 올라오는 미가공 날것의 원시 문자열을 누적 적재하는 로컬 버퍼입니다.
        let mut raw_buffer = String::new();
        // 예외 발생을 점검하는 스레드 간 동기화 확인 변수입니다.
        let mut has_error = false;
        // 에러 상세 사유 문안 보존지입니다.
        let mut error_msg = None;
        // 클라이언트(사용자 UI) 측으로 실시간 스트리밍 중계를 차단 정지시킬지 제어하는 가드 변수입니다.
        let mut client_stream_stopped = false;
        // 이미 사용자 화면에 송출해 보낸 텍스트 글자 수 위치 기준점입니다.
        let mut streamed_len = 0usize;
        
        // 콜백 스레드가 채널 파이프라인(rx)으로 송출하는 중간 토큰 패킷들을 꺼내 차례로 조율합니다.
        while let Some(msg) = rx.blocking_recv() {
            // 수신 패킷 타입 매칭
            match msg {
                // 걸러진 텍스트 조각을 수령한 경우
                StreamMessage::Chunk(c) => {
                    // 전체 종합 텍스트 판에 수신 문자열 조각을 가산 축적합니다.
                    full_response_content.push_str(&c);
                    
                    // 도구 호출 키워드(tool_calls 등)가 들어가면 모델의 도구 실행용 구문으로 판정되므로 텍스트 스트리밍을 은폐합니다.
                    let has_tool_keyword = full_response_content.contains("\"tool_calls\"") || full_response_content.contains("tool_call");

                    // 만약 모델이 대답 마지막에 자가 입력을 요구하는 [USER_INPUT] 표식을 넣으면 사용자 화면 스트리밍을 정지합니다.
                    let mut visible_end = full_response_content.len();
                    // 표식의 최초 발생 지점을 검색합니다.
                    if let Some(pos) = full_response_content.find("[USER_INPUT]") {
                        // 스트리밍 기준 범위를 해당 표식 전까지만으로 슬라이싱합니다.
                        visible_end = pos;
                    }

                    // 도구 구문 진행이 아니며, 스트리밍 차단 상태도 아니고, 새로 추가된 사용자 가독 텍스트가 있다면 클라이언트에 송출합니다.
                    if !has_tool_keyword && !client_stream_stopped && visible_end > streamed_len {
                        // 실시간 신규 출력 글자 범위만 정확하게 잘라냅니다.
                        if let Some(visible_chunk) = full_response_content.get(streamed_len..visible_end) {
                            // 신규 글자 조각을 클라이언트 전송 채널(event_tx)에 격발해 보냅니다.
                            let _ = event_tx.send(ServerStreamEvent::Chunk(visible_chunk.to_string()));
                        }
                        // 송출 완료된 글자 오프셋 길이를 갱신해 다음 전송 시 중복 출력을 차단합니다.
                        streamed_len = visible_end;
                    }

                    // 자가 재귀 채팅 태그([USER_INPUT])를 확실히 읽었다면 실시간 스트리밍은 마감 통제합니다.
                    if full_response_content.contains("[USER_INPUT]") {
                        // 송출 금지 상태로 가드 처리
                        client_stream_stopped = true;
                    }
                }
                // FFI 로우 데이터를 수신했을 시 원시 데이터를 업데이트해 둡니다.
                StreamMessage::RawBuffer(r) => {
                    // 원시값 백업
                    raw_buffer = r;
                }
                // 비동기 스레드로부터 완료 지시서가 수령되었을 때
                StreamMessage::Final { has_error: err, error_msg: msg_err, .. } => {
                    // 에러 여부 마킹
                    has_error = err;
                    // 상세 오류 내용 이양
                    error_msg = msg_err;
                    // 수신 대기 수렁에서 이탈해 다음 처리를 개시합니다.
                    break;
                }
            }
        }
        
        // 현재 추론 턴 동안 누적 소모/생성한 물리적 토큰 개수 통계를 C API 연산으로 최종 정합 수거합니다.
        let (p_tok, c_tok) = get_conversation_token_counts(conversation);
        // 총 누적 입력 토큰 가산
        total_prompt_tokens += p_tok;
        // 총 누적 생성 토큰 가산
        total_completion_tokens += c_tok;
        
        // FFI 메모리 안전을 위해 안전 조치와 함께 C 대화 인스턴스 자원을 소거하고, 박제했던 송신기 메모리도 완전히 삭제 처리합니다.
        unsafe {
            // 대화 삭제 C API 호출
            sys::litert_lm_conversation_delete(conversation);
            // Box 회수를 통한 원천 누수 봉쇄 및 스레드 자원 소거
            let _ = Box::from_raw(tx_ptr);
        }
        
        // 만일 FFI 하부에서 심각한 오류가 전염되어 돌아왔다면 해당 에러를 클라이언트에 전파하고 에이전트 공정을 조기 종결합니다.
        if has_error {
            // 오류명 빌드
            let err_str = error_msg.unwrap_or_else(|| "Unknown FFI stream error".to_string());
            // 에러 공지 이벤트 격발
            let _ = event_tx.send(ServerStreamEvent::Error(err_str));
            // 루프 이탈
            break;
        }
        
        // 모델이 결정한 도구 호출 규격이 탐색되었는지 집계할 로컬 임시 배열입니다.
        let mut tool_calls_info = Vec::new();
        // 도구 목록 정보를 기록할 JSON 밸류 객체입니다.
        let mut calls_json = serde_json::Value::Null;
        
        // 감지된 도구 호출 JSON 구조를 백업할 변수입니다.
        let mut detected_tool_calls = String::new();
        // 모델 원시 텍스트 버퍼 내에 JSON 규격 키워드("tool_calls")가 탑재되었는지 검사합니다.
        if raw_buffer.contains("\"tool_calls\"") {
            // 해당 로우 데이터가 실제 유효한 JSON으로 온전히 복구 변환되는지 해석합니다.
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&raw_buffer) {
                // 내부에 tool_calls 키가 정확하게 명시되어 있다면 유효 감지로 책정합니다.
                if j.get("tool_calls").is_some() {
                    // 원시 텍스트 백업
                    detected_tool_calls = raw_buffer.clone();
                }
            }
        }
        
        // 1유형: 모델이 정규 C API tool_call 사양에 밀착하여 완벽한 JSON 프로토콜 구조로 호출을 시도했을 때의 처리입니다.
        if !detected_tool_calls.is_empty() {
            // 문법 해석을 수행합니다.
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&detected_tool_calls) {
                // 내부 tool_calls 배열 필드를 획득합니다.
                if let Some(calls_arr) = j.get("tool_calls").and_then(|v| v.as_array()) {
                    // 리스트를 JSON 데이터 양식으로 보존해 둡니다.
                    calls_json = serde_json::json!(calls_arr);
                    // 배열을 풀며 내부 호출 지시서들을 낱개 분석합니다.
                    for call in calls_arr {
                        // 고유 거래 ID(id) 값을 가져오며 분실 시 미상으로 채워 줍니다.
                        let c_id = call.get("id").and_then(|v| v.as_str()).unwrap_or("call_unknown").to_string();
                        // 실행할 대상 도구 명칭
                        let mut f_name = String::new();
                        // 실행 매개변수 값
                        let mut f_args = String::new();
                        // 세부 function 프로토콜 블록이 존재하는지 조사합니다.
                        if let Some(func) = call.get("function") {
                            // 실행 도구의 실명을 추출합니다.
                            f_name = func.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            // 매개변수 arguments 필드를 확보합니다.
                            if let Some(args) = func.get("arguments") {
                                // 파라미터가 이미 문자열 형식이면 소유권을 넘기고, 객체 타입이라면 스트링으로 직렬화해 옵니다.
                                if let Some(args_str) = args.as_str() {
                                    // 문자열 적용
                                    f_args = args_str.to_string();
                                } else {
                                    // 직렬화 치환
                                    f_args = args.to_string();
                                }
                            }
                        }
                        // 기동 목록에 도구 이름, 매개변수 본문, 고유 거래 ID를 기재합니다.
                        tool_calls_info.push((f_name, f_args, c_id));
                    }
                }
            }
        } else {
            // 2유형: 정형 API 양식 대신 본문 텍스트 내부에 커스텀 마크업(<|tool_call>)이나 플랫 JSON 포맷으로 어설프게 도구를 기동했을 때입니다.
            let parsed_calls = parse_all_custom_tool_calls(&full_response_content);
            // 비정형 감지기가 무언가 유효한 호출 양식을 파싱해낸 경우
            if !parsed_calls.is_empty() {
                // 규격을 맞춘 도구 리스트 보관 배열을 준비합니다.
                let mut calls_arr = Vec::new();
                // 찾아낸 가짜 도구 목록을 돌면서, 표준 통신이 가능하도록 정규 API 양식으로 역조립(mocking)합니다.
                for (i, (f_name, f_args_val)) in parsed_calls.into_iter().enumerate() {
                    // 고유 거래 ID를 난수와 타임스탬프 기반으로 임의 복원 작성해 줍니다.
                    let c_id = format!("call_manual_{}_{}", get_iso8601_now().replace(":", "_").replace("-", "_").replace(".", "_"), i);
                    // 변수 JSON을 문자열로 덤프 획득합니다.
                    let f_args_str = f_args_val.to_string();
                    // 역직렬화하여 정규 API의 데이터 모습으로 포맷화해 배열에 탑재시킵니다.
                    calls_arr.push(serde_json::json!({
                        "id": c_id.clone(),
                        "type": "function",
                        "function": {
                            "name": f_name.clone(),
                            "arguments": f_args_str.clone()
                        }
                    }));
                    // 기동 리스트에도 수동 복조한 내역을 등록합니다.
                    tool_calls_info.push((f_name, f_args_str, c_id));
                }
                // 가공 완료된 최종 JSON 목록을 덤프합니다.
                calls_json = serde_json::json!(calls_arr);
            }
        }
        
        // 스캔 결과 동작해야 할 기능 도구들이 존재한다면 에이전트 루프 연쇄 공정을 진행합니다.
        if !tool_calls_info.is_empty() {
            // 도구 실행 내역을 담은 JSON 통신문을 준비합니다.
            let raw_tc_json = serde_json::json!({
                "tool_calls": calls_json
            }).to_string();
            // 클라이언트에게 지금부터 특정 도구를 기동함을 실시간 스트림 이벤트로 발송합니다.
            let _ = event_tx.send(ServerStreamEvent::ToolCall {
                // 원시 호출 JSON 통보
                raw_tool_calls_json: raw_tc_json,
            });
            
            // 현재 활성 메시지와 유저 원문 본문이 동일하다면(에이전트 연쇄 첫 턴), 사용자 질문을 대화 역사 목록에 백업 편입시킵니다.
            if active_msg == current_msg {
                // 원문 질의가 순수한 JSON 규격으로 파싱 가능한지 진단합니다.
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&current_msg) {
                    // 파싱 완료된 질의 오브젝트 추가
                    local_history.as_array_mut().unwrap().push(parsed);
                } else {
                    // 일반 텍스트 포맷 구조인 경우 형식을 복원 기재하여 주입합니다.
                    local_history.as_array_mut().unwrap().push(serde_json::json!({
                        "role": "user",
                        "content": current_msg
                    }));
                }
            } else {
                // 연쇄 진행 2턴 이상인 경우, 직전까지 진행되어 활성화된 도구 결과 메시지 객체를 대화 역사에 탑재합니다.
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&active_msg) {
                    // 역사에 저장
                    local_history.as_array_mut().unwrap().push(parsed);
                }
            }
            
            // 모델이 도구를 기동했다는 인공지능 측 통화 내역("role": "assistant")을 대화 역사 목록에 세팅합니다.
            local_history.as_array_mut().unwrap().push(serde_json::json!({
                "role": "assistant",
                "content": full_response_content,
                // 기동 사양서 보존
                "tool_calls": calls_json
            }));
            
            // 동작시켜야 하는 전체 도구의 총 수량을 헤아립니다.
            let num_calls = tool_calls_info.len();
            // 개별 도구 사양 정보들을 튜플로 돌리며 실 동작을 가동합니다.
            for (index, (func_name, func_args, call_id)) in tool_calls_info.into_iter().enumerate() {
                // 만일 격발할 도구가 메타 도구인 "create_or_update_tool"인 경우의 사용자 안내 메시지 가공 과정입니다.
                if func_name == "create_or_update_tool" {
                    // 기본 설명들을 초기화 세팅
                    let mut tool_name = "알 수 없음".to_string();
                    let mut tool_desc = "설명 없음".to_string();
                    // 파라미터 내역을 읽어서 생성할 도구의 이름과 설명글을 찾아냅니다.
                    if let Ok(args_val) = serde_json::from_str::<serde_json::Value>(&func_args) {
                        // 명칭 획득
                        if let Some(n) = args_val.get("name").and_then(|v| v.as_str()) {
                            tool_name = n.to_string();
                        }
                        // 설명 확보
                        if let Some(d) = args_val.get("description").and_then(|v| v.as_str()) {
                            // 비어있지 않음 확인
                            if !d.trim().is_empty() {
                                tool_desc = d.to_string();
                            }
                        }
                    }
                    // 메타 툴의 성격에 적합한 진행 가이드 대사("도구를 만듭니다...")를 구축합니다.
                    let guidance = format!(
                        "도구를 만듭니다. 이름: {}. 목적: {}",
                        tool_name, tool_desc
                    );
                    // 클라이언트 실시간 화면에 가이드 지침 메시지 이벤트를 송출합니다.
                    let _ = event_tx.send(ServerStreamEvent::Guidance(guidance));
                } else {
                    // 메타 툴이 아닌 일반 유틸리티 도구를 구동할 때의 안내 대사 가공부입니다.
                    let guidance = format!(
                        "도구를 사용합니다. 이름: {}",
                        func_name
                    );
                    // 일반 도구 기동 진행률 알림을 클라이언트에 방출합니다.
                    let _ = event_tx.send(ServerStreamEvent::Guidance(guidance));
                }
                
                // 해당 도구를 실제 격발 구동하여 파이썬 콘솔 등의 출력 최종 문자열을 받아냅니다.
                let mut tool_result = execute_tool(&func_name, &func_args);
                
                // 실행 출력본에 에러 표식(Error, Traceback 등)이 묻어 나오는 경우 자가 교정 및 피드백 명령을 꼬리에 붙여 줍니다.
                if tool_result.contains("Error")
                    || tool_result.contains("error")
                    || tool_result.contains("실패")
                    || tool_result.contains("Traceback")
                {
                    // 모델이 문법을 즉각 수정하여 자율 재시도할 수 있도록 주입하는 내부 조율용 지침 텍스트입니다.
                    tool_result.push_str("\n\n[시스템 자동 지시] 위 도구 실행이 실패했습니다. 오류 분석이나 설명 텍스트를 절대 출력하지 마십시오. 즉시 create_or_update_tool 도구 호출 JSON만 출력하여 수정된 코드를 재등록하고, 이어서 해당 도구를 다시 호출하십시오. 텍스트를 한 글자라도 출력하면 도구 호출이 실패합니다.");
                }
                
                // 실행에 성공하여 돌아온 도구 표준 출력값을 스트림 전송에 맞추어 클라이언트에 송달합니다.
                let _ = event_tx.send(ServerStreamEvent::ToolResult {
                    name: func_name.clone(),
                    result: tool_result.clone(),
                });
                
                // 대화 내역에 편입시키기 위한 정규 도구 반응 구조체("role": "tool")를 구성합니다.
                let tool_msg = serde_json::json!({
                    "role": "tool",
                    "name": func_name,
                    "tool_call_id": call_id,
                    "content": tool_result
                });
                
                // 여러 도구들의 일괄 동작 시 꼬리가 물릴 때의 처리입니다.
                if index + 1 < num_calls {
                    // 아직 실행할 도구들이 뒤에 더 남아있다면, 즉시 역사 배열에 차곡차곡 쌓아둡니다.
                    local_history.as_array_mut().unwrap().push(tool_msg);
                } else {
                    // 가장 최후에 끝난 도구의 결과 텍스트는 다음 추론 루프의 격발 입력 메시지(active_msg)로 갱신하여 덮어씁니다.
                    active_msg = tool_msg.to_string();
                }
            }
            // 도구가 동작했으므로 loop_count 1회 소모 후 새로운 추론 턴을 시작하기 위해 루프 최상단으로 컨티뉴합니다.
            continue;
        }
        
        // 3유형: 도구 격발 지시가 일절 감지되지 않은 순수 텍스트 답변이 생성되었을 때의 처리입니다.
        if tool_calls_info.is_empty() {
            // 모델이 스스로 유저 역극 채팅을 요구하는 특수 기호 태그들을 감지합니다.
            let user_input_tag = if full_response_content.contains("[USER_INPUT]") {
                // 정규 태그 매칭
                Some(("[USER_INPUT]", "[USER_INPUT]".len()))
            } else if full_response_content.contains("[USER_INPUT") {
                // 괄호 누락형 깨진 태그 매칭
                Some(("[USER_INPUT", "[USER_INPUT".len()))
            } else {
                // 미발견
                None
            };

            // 자가 태그가 정밀 색출되었을 경우
            if let Some((tag, tag_len)) = user_input_tag {
                // 텍스트 상에서 감지된 태그의 첫 위치 인덱스를 취득합니다.
                let pos = full_response_content.find(tag).unwrap();
                // 태그 이후 영역에 남겨진 AI의 내부 자가 유저 역극 메시지 본안을 잘라냅니다.
                let mut prompt_content = full_response_content[pos + tag_len..].to_string();
                
                // 만일 깨진 괄호 태그 구조 탓에 접두 흔적 문자(']')가 서두에 묻어 있다면 슬라이스하여 제거합니다.
                if tag == "[USER_INPUT" && prompt_content.starts_with(']') {
                    // 한 칸 가위질
                    prompt_content = prompt_content[1..].to_string();
                }
                
                // 지시가 무한히 늘어지는 걸 차단하기 위해 마감 태그([END_INPUT])를 탐지해 검출 범위를 통제합니다.
                let end_tag = if prompt_content.contains("[END_INPUT]") {
                    // 정규 마감
                    Some("[END_INPUT]")
                } else if prompt_content.contains("[END_INPUT") {
                    // 미마감 감지
                    Some("[END_INPUT")
                } else {
                    // 없음
                    None
                };
                
                // 마감 기호가 탐색된 상태라면
                if let Some(e_tag) = end_tag {
                    // 마감 기호 이전 영역까지만 알맹이 지시 본문으로 한정 수용하고 나머지는 잘라냅니다.
                    if let Some(end_pos) = prompt_content.find(e_tag) {
                        // 슬라이스
                        prompt_content = prompt_content[..end_pos].to_string();
                    }
                }
                
                // 유저 입력에 담길 잔여 공백을 깨끗이 날려줍니다.
                let prompt_content = prompt_content.trim().to_string();
                
                // 잘라낸 자가 명령어가 비어있지 않다면, 재귀 루프 재시동 프로세스로 돌입시킵니다.
                if !prompt_content.is_empty() {
                    // 디버그 콘솔창에 AI가 스스로 사용자 키보드를 타이핑하여 전방 선언한 내용을 출력 보고합니다.
                    println!("[시스템] [재귀 실행] AI가 스스로 USER 채팅을 입력했습니다: {}", prompt_content);
                    // 태그 문자열 이전까지 작성한 순수 답변 텍스트만 떼어내 정밀화합니다.
                    let cleaned_assistant_content = full_response_content[..pos].trim().to_string();
                    
                    // 첫 회차 유동 검사
                    if active_msg == current_msg {
                        // 사용자 대화를 역사에 등기
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&current_msg) {
                            local_history.as_array_mut().unwrap().push(parsed);
                        } else {
                            // 원본 확보 포맷 기입
                            local_history.as_array_mut().unwrap().push(serde_json::json!({
                                "role": "user",
                                "content": current_msg
                            }));
                        }
                    } else {
                        // 진행 중이던 이전 동작 패킷을 역사 목록에 반영
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&active_msg) {
                            local_history.as_array_mut().unwrap().push(parsed);
                        }
                    }
                    
                    // 태그 이전까지 작성한 어시스턴트의 답변 텍스트를 대화 역사에 등재합니다.
                    local_history.as_array_mut().unwrap().push(serde_json::json!({
                        "role": "assistant",
                        "content": cleaned_assistant_content
                    }));
                    
                    // 모델이 자가 입력한 질의 프레임을 신규 사용자 턴("role": "user")으로 포장하여 대화 입력 메시지로 준비합니다.
                    let new_user_msg = serde_json::json!({
                        "role": "user",
                        "content": prompt_content
                    });
                    // active_msg에 인가하고 즉시 다음 추론을 기동하기 위해 상단 루프로 컨티뉴합니다.
                    active_msg = new_user_msg.to_string();
                    // 재시동 컨티뉴
                    continue;
                }
            }
            
            // 모델의 자가 재귀 지시서가 없는 온전하고 최종적인 답변 완성 상황일 때
            // 에이전트 연쇄 첫 턴인 경우
            if active_msg == current_msg {
                // 유저 원래의 발문을 대화 역사에 최종 기록합니다.
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&current_msg) {
                    local_history.as_array_mut().unwrap().push(parsed);
                } else {
                    // 일반 등기
                    local_history.as_array_mut().unwrap().push(serde_json::json!({
                        "role": "user",
                        "content": current_msg
                    }));
                }
            } else {
                // 잔여 임시 액티브 메시지 내용을 역사에 추가합니다.
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&active_msg) {
                    local_history.as_array_mut().unwrap().push(parsed);
                }
            }
            // 모델이 답변을 완수하여 생성해낸 텍스트 총량을 대화 역사에 등기합니다.
            local_history.as_array_mut().unwrap().push(serde_json::json!({
                "role": "assistant",
                "content": full_response_content.clone()
            }));
        }
        
        // 모든 체인 동작이 종료되었으므로 10회 하한 루프를 완전히 탈출합니다.
        break;
    }
    
    // 로컬 히스토리 JSON 데이터를 Rust 벡터 구조형태로 덤프하여 분양해옵니다.
    let final_history_vec = local_history.as_array().cloned().unwrap_or_default();
    // 메인 API 라우트 스레드에게 생성이 총괄 완성되었음을 토큰 소비 통계와 함께 Done 이벤트로 최종 통보합니다.
    let _ = event_tx.send(ServerStreamEvent::Done {
        final_history: final_history_vec,
        prompt_tokens: total_prompt_tokens,
        completion_tokens: total_completion_tokens,
    });
}
