// 비동기 스레드(네트워크 통신 태스크)와 메인 UI 스레드 간 데이터 통신을 위해 Tokio의 mpsc 채널을 임포트합니다.
use tokio::sync::mpsc;
// JSON 구조체 생성용 json 매크로 및 데이터 처리를 위해 Value 객체를 가져옵니다.
use serde_json::{json, Value};
// 서버 측으로 HTTP POST 요청을 날리기 위해 reqwest의 Client 라이브러리를 사용합니다.
use reqwest::Client;
// 비동기 패킷 스트림 수령 처리를 위해 StreamExt 트레이트를 임포트합니다.
use futures_util::StreamExt;
// 파일 및 경로 존재 유무 점검을 위해 Path 표준 라이브러리를 임포트합니다.
use std::path::Path;

// 설정 로드/세이브 및 상대 경로 치환을 위해 settings 모듈 함수들을 가져옵니다.
use crate::settings::{ChatSettings, expand_path, save_settings};
// 수신 이벤트 통지 팩과 개행 파서, 태그 정제 유틸리티를 events 모듈로부터 가져옵니다.
use crate::events::{NetworkEvent, StreamParser, clean_tags};

// TUI 화면 내부에서 키보드 입력 스크롤 포커스가 어떤 창(Pane)에 할당되어 있는지 표기하는 분기용 열거형입니다.
#[derive(Clone, Copy, PartialEq)]
pub enum ActivePane {
    // 일반 대화 히스토리 창입니다.
    Chat,
    // 자율 도구 실행 및 시스템 상세 안내 로그를 출력하는 모니터링 창입니다.
    Recursive,
    // 작업 계획 및 작업 진행 상태표를 가시화하는 대시보드 판입니다.
    Board,
    // 도구 생성 및 실행 콘솔 로우 출력 기록창입니다.
    Logs,
}

// TUI 전체 화면의 UI 상태 변수들과 통신 상태, 그리고 설정 데이터들을 총괄 파이프라인 관리하는 컨트롤러 구조체입니다.
pub struct TuiApp {
    // 사용자가 현재 키보드로 기입 중인 텍스트 입력창 버퍼입니다.
    pub input: String,
    // (역할, 대화 본문)의 쌍으로 구성된 역사적 채팅 다이어그램 목록입니다.
    pub chat_history: Vec<(String, String)>,
    // LLM 모델이 현재 실시간으로 찍어내 조립 중인 실시간 응답 텍스트 임시 메모리입니다.
    pub current_assistant_response: String,
    // 에이전트 자율 진행 상태 항목들을 저장해 두는 목록 벡터입니다.
    pub agent_status: Vec<String>,
    // 에이전트가 조립 수립한 작업 계획 리스트를 관리하는 벡터입니다.
    pub agent_plan: Vec<String>,
    // 도구 실행 및 파이썬 로우 아웃풋의 누적 로그 리스트입니다.
    pub tool_logs: Vec<String>,
    // 에이전트가 도구를 호출하고 응답을 받는 등 전 과정을 역동적으로 기록하는 타임라인 벡터입니다.
    pub recursive_logs: Vec<String>,
    // LLM 추론 시 첨부 파일 형식으로 전송 대기 중인 이미지 로컬 물리 주소입니다.
    pub pending_image: Option<String>,
    // 현재 백그라운드에서 추론 연산 처리가 비동기로 대기/동작 중인지를 표기하는 로딩 상태 스위치입니다.
    pub is_loading: bool,
    
    // 현재 활성화되어 상하 방향키 스크롤을 접수할 타깃 제어 창 분기 값입니다.
    pub active_pane: ActivePane,
    // 대화 역사 창의 스크롤 위치 제어 인덱스 값입니다.
    pub chat_scroll: u16,
    // 자율 도구 타임라인 창의 스크롤 제어 오프셋 값입니다.
    pub recursive_scroll: u16,
    // 작업 진행판 창의 스크롤 인덱스 조절 변수입니다.
    pub board_scroll: u16,
    // 로우 실행 로그 창의 스크롤 위치 변수입니다.
    pub logs_scroll: u16,
    
    // 타깃 통신을 수행할 대상 서버 호스트 주소 규격 문자열(HTTP URL)입니다.
    pub server_url: String,
    // 현재 조율되어 로드된 하이퍼파라미터 설정 인스턴스입니다.
    pub settings: ChatSettings,
    // API 질의 시 함께 실어 넘길 누적 히스토리 JSON Value 배열의 사본입니다.
    pub history_json_list: Vec<Value>,
    // 바로 직전 추론 턴에서 소비된 입력 토큰 계수 통계값입니다.
    pub last_prompt_tokens: i32,
    // 바로 직전 생성 턴에서 만들어진 출력 토큰 계수 통계값입니다.
    pub last_completion_tokens: i32,
}

// TuiApp 상태 제어기에 속한 비즈니스 로직 함수군을 전개합니다.
impl TuiApp {
    // 셸 CLI 부팅 시 접수한 포트 번호 및 환경 매개변수를 바탕으로 TuiApp 인스턴스를 초깃값으로 정합 생성합니다.
    pub fn new(settings: ChatSettings, server_url: String) -> Self {
        // 모든 필드 요소를 안정적인 비어 있는 상태 및 설정값으로 채워 인스턴스화합니다.
        Self {
            // 사용자 입력창 초기화
            input: String::new(),
            // 대화 목록 청소
            chat_history: Vec::new(),
            // 실시간 버퍼 청소
            current_assistant_response: String::new(),
            // 에이전트 상태판 비우기
            agent_status: Vec::new(),
            // 작업 계획 리스트 비우기
            agent_plan: Vec::new(),
            // 로그 버퍼 초기화
            tool_logs: Vec::new(),
            // 타임라인 내역 초기화
            recursive_logs: Vec::new(),
            // 대기 이미지 초기값 부재 설정
            pending_image: None,
            // 비가동 상태 표기
            is_loading: false,
            // 첫 키보드 포커스는 채팅창으로 설정
            active_pane: ActivePane::Chat,
            // 스크롤 인덱스 일괄 0번 지정
            chat_scroll: 0,
            // 스크롤 0번 지정
            recursive_scroll: 0,
            // 스크롤 0번 지정
            board_scroll: 0,
            // 스크롤 0번 지정
            logs_scroll: 0,
            // 타깃 서버 주소 복사
            server_url,
            // 조립된 설정 인계
            settings,
            // 역사 저장소 세팅
            history_json_list: Vec::new(),
            // 토큰 통계 초기화
            last_prompt_tokens: 0,
            // 토큰 통계 초기화
            last_completion_tokens: 0,
        }
    }

    // 슬래시(/) 문장 기호로 시작되는 특수 셸 제어 명령어(Command)들을 인터프리팅 조율 처리하는 전담 함수입니다.
    pub fn handle_command(&mut self, cmd: &str) {
        // 앞뒤 공백을 절단해 명령어 원형을 정돈합니다.
        let trimmed = cmd.trim();
        // 화면과 대화 상태를 말끔히 비우는 "/clear" 명령어인지 분기합니다.
        if trimmed == "/clear" {
            // 대화 내역 전체 정리
            self.chat_history.clear();
            // 어시스턴트 임시 버퍼 소거
            self.current_assistant_response.clear();
            // 에이전트 현황 제거
            self.agent_status.clear();
            // 계획 단계 삭제
            self.agent_plan.clear();
            // 로그 제거
            self.tool_logs.clear();
            // 타임라인 삭제
            self.recursive_logs.clear();
            // 첨부 이미지 대기 해제
            self.pending_image = None;
            // 직렬화 전송 히스토리 완벽 소거
            self.history_json_list.clear();
            // 대시보드 갱신 안내 알림 추가
            self.tool_logs.push("💡 대화 기록과 대시보드가 초기화되었습니다.".to_string());
        } else if trimmed.starts_with("/img ") {
            // 이미지 주소를 첨부하는 "/img <경로>" 명령어의 파싱 부분입니다.
            let path = trimmed[5..].trim();
            // 틸데 물결 기호를 절대 주소 홈 디렉토리 경로로 강제 확장 환원시킵니다.
            let resolved = expand_path(path);
            // 해당 디렉토리 주소지에 타깃 물리 파일이 정상 부합해 생존하는지 진단합니다.
            if Path::new(&resolved).exists() {
                // 대기 슬롯에 이미지 경로 보존 기재
                self.pending_image = Some(resolved.clone());
                // TUI 로그창에 첨부 성공을 보고합니다.
                self.tool_logs.push(format!("🖼 이미지 첨부 완료: {}", resolved));
            } else {
                // 파일 부재 시 경보 로그 출력
                self.tool_logs.push(format!("❌ 이미지 파일을 찾을 수 없음: {}", resolved));
            }
        } else if trimmed.starts_with("/temp ") {
            // 온도 계수를 변경하는 "/temp <수치>" 명령어 처리 파트입니다.
            if let Ok(val) = trimmed[6..].trim().parse::<f32>() {
                // 설정 데이터의 인자 갱신
                self.settings.temperature = val;
                // 로그창 알림 출력
                self.tool_logs.push(format!("⚙️ Temperature 변경: {}", val));
                // 설정 저장소 덤프 갱신
                save_settings(&self.settings);
            }
        } else if trimmed.starts_with("/top_p ") {
            // spec top_p 누적 확률 비율 조절 파트입니다.
            if let Ok(val) = trimmed[7..].trim().parse::<f32>() {
                // 설정 반영
                self.settings.top_p = val;
                // 로그창 보고
                self.tool_logs.push(format!("⚙️ Top-P 변경: {}", val));
                // 영구 파일 기록
                save_settings(&self.settings);
            }
        } else if trimmed.starts_with("/top_k ") {
            // spec top_k 후보 상한 조절 파트입니다.
            if let Ok(val) = trimmed[7..].trim().parse::<u32>() {
                // 설정 갱신
                self.settings.top_k = val;
                // 로그 표기
                self.tool_logs.push(format!("⚙️ Top-K 변경: {}", val));
                // 설정 파일 보존
                save_settings(&self.settings);
            }
        } else if trimmed.starts_with("/max_tokens ") {
            // spec 최대 토큰 생성 한도 한계 조절 파트입니다.
            if let Ok(val) = trimmed[12..].trim().parse::<u32>() {
                // 토큰 상한 갱신
                self.settings.max_tokens = val;
                // 로그창 등기
                self.tool_logs.push(format!("⚙️ Max Tokens 변경: {}", val));
                // 디스크 저장
                save_settings(&self.settings);
            }
        } else if trimmed.starts_with("/prompt ") {
            // spec 시스템 지침 강제 임의 수동 덮어쓰기 파트입니다.
            let prompt = trimmed[8..].trim().to_string();
            // 통합 프롬프트 변수 교체
            self.settings.system_prompt = prompt.clone();
            // 동기화 로그 등기
            self.tool_logs.push(format!("⚙️ System Prompt 변경 (총 {}자)", prompt.len()));
            // 영구 갱신
            save_settings(&self.settings);
        } else {
            // 명령어 문구가 식별 불가 시 에러 로그로 회신 처리합니다.
            self.tool_logs.push(format!("❌ 알 수 없는 명령어: {}", trimmed));
        }
    }

    // 사용자가 타자한 메일 질의 내용을 서버 측 비동기 스트림 인터페이스로 발송 요청하는 기동 함수입니다.
    pub fn send_message(&mut self, msg: String, net_tx: mpsc::Sender<NetworkEvent>) {
        // 이미 연산 작업이 굴러가는 로딩 상태라면 중복 접수를 강제 차단합니다.
        if self.is_loading {
            return;
        }
        // 에이전트 작동 온 플래그 활성화
        self.is_loading = true;
        // 기존 조립용 임시 아웃풋 문자열 비우기
        self.current_assistant_response.clear();
        
        // 이미지 파일이 동봉 첨부되어 대기 중인지 비교 진단하여 JSON 콘텐츠 규격을 분기 형성합니다.
        let content_val = match self.pending_image.take() {
            // 이미지 존재 시 멀티모달 배열 규약(Array) 구조로 포맷 조립합니다.
            Some(img_path) => {
                // 이미지 명세 객체를 구성하여 배열에 첫 타자로 선점 주입시킵니다.
                let mut parts = vec![json!({"type": "image", "path": img_path})];
                // 텍스트 지문도 곁들여 들어왔는지 확인하여 뒤이어 융합합니다.
                if !msg.is_empty() {
                    // 텍스트 블록 추가
                    parts.push(json!({"type": "text", "text": msg}));
                }
                // 배열 형태 밸류 적용
                Value::Array(parts)
            }
            // 일반 대화 텍스트인 경우 평문형 스트링 밸류로 직행 적용합니다.
            None => Value::String(msg.clone()),
        };

        // 전송 규격에 맞추어 유저 역할명(role) 아래 콘텐츠 객체(content)를 포장합니다.
        let user_msg = json!({"role": "user", "content": content_val});
        // API 연속 호출 역사를 유지하기 위해 리스트 목록에 본문을 편입 등기합니다.
        self.history_json_list.push(user_msg.clone());
        // TUI 전방 화면의 유저 채팅 이력판에도 타자 내용을 적재합니다.
        self.chat_history.push(("user".to_string(), msg));

        // 백그라운드 네트워크 처리를 전개하기 위해 클라이언트를 복제 배정합니다.
        let client = Client::new();
        // 타깃 접속 서버 주소 복사
        let server_url = self.server_url.clone();
        // 하이퍼파라미터 설정 팩 복제
        let settings = self.settings.clone();
        // 직렬화 역사 리스트 복사
        let history = self.history_json_list.clone();

        // 메인 UI 렌더링 스레드의 프레임 드롭을 방지하기 위해 완전히 비동기 백그라운드 작업(tokio::spawn)을 가동합니다.
        tokio::spawn(async move {
            // 최초 기점으로 성격 설정 프롬프트를 전방에 등기시킵니다.
            let mut request_messages = vec![json!({
                "role": "system",
                "content": settings.system_prompt
            })];
            // 직전 라운드까지 진행되었던 대화 발문들을 뒤에 연속적으로 가산합니다.
            request_messages.extend(history.clone());

            // 서버 측 Ollama 호환용 chat 엔드포인트 파라미터 규격을 작성합니다.
            let request_payload = json!({
                "model": "litert-lm:latest",
                "messages": request_messages,
                // 실시간 NDJSON 스트리밍 방식으로 수신을 강제 활성화합니다.
                "stream": true,
                // 튜닝 인자값 조율
                "options": {
                    "temperature": settings.temperature,
                    "top_p": settings.top_p,
                    "top_k": settings.top_k,
                    "max_output_tokens": settings.max_tokens
                }
            });

            // HTTP POST 통신을 비동기로 개시합니다.
            let response = match client.post(format!("{}/api/chat", server_url))
                .json(&request_payload) // 페이로드 적재
                .send() // 발송
                .await // 응답 대기
            {
                // 응답 무사 수령 성공 시 객체 양도
                Ok(resp) => resp,
                // 소켓 접속 불가 등 통신 이상 상황 시 예외 처리
                Err(e) => {
                    // 에러 채널을 통해 긴급 이상 통지 이벤트를 송출하고 비동기 스레드를 퇴거합니다.
                    let _ = net_tx.send(NetworkEvent::Error(format!("통신 오류: {}", e))).await;
                    return;
                }
            };

            // HTTP 상태 코드가 200번 정상 코드가 아닌 에러 응답인 경우의 예외 처리입니다.
            if !response.status().is_success() {
                // 회신된 상세 에러 본안을 수거합니다.
                let err_text = response.text().await.unwrap_or_default();
                // 서버 장애 경보를 이벤트를 통해 송출하고 종료합니다.
                let _ = net_tx.send(NetworkEvent::Error(format!("서버 오류: {}", err_text))).await;
                return;
            }

            // 회신 바이트 데이터를 청크 스트림 단위로 흘려받아 해석을 감시합니다.
            let mut stream = response.bytes_stream();
            // 개행 문장 파싱을 위한 백그라운드 문자 수집 버퍼를 준비합니다.
            let mut line_buffer = String::new();
            // 수집한 전체 어시스턴트의 최종 원본 대답 본체를 담을 메모리 버퍼입니다.
            let mut full_response_content = String::new();
            // 감지된 모델의 도구 기동 JSON 구조체를 기억 보관할 Option형 변수입니다.
            let mut tool_calls: Option<Value> = None;
            // 서버 측에서 병합 완료하여 Done 턴에 보강 보고해 준 전체 역사 목록 보관지입니다.
            let mut server_history: Option<Vec<Value>> = None;
            // 스트림 정제 파서 모듈 인스턴스를 소환합니다.
            let mut parser = StreamParser::new();
            // 입력 토큰 수집 카운터
            let mut prompt_tokens = 0i32;
            // 출력 토큰 수집 카운터
            let mut completion_tokens = 0i32;

            // 소켓 파이프로부터 다음 바이트 청크 조각이 분출되어 공급되는 동안 대기 수집합니다.
            while let Some(chunk_res) = stream.next().await {
                // 바이트 변환 이상을 감시합니다.
                let chunk = match chunk_res {
                    // 수령 완료
                    Ok(bytes) => bytes,
                    // 통신 도중 절단 장애 예외 처리
                    Err(e) => {
                        // 스트림 중단 이벤트를 전파하고 이탈합니다.
                        let _ = net_tx.send(NetworkEvent::Error(format!("스트림 에러: {}", e))).await;
                        return;
                    }
                };

                // 받아낸 바이트 배열을 UTF-8 평문형 텍스트로 치환 보정합니다.
                let chunk_str = String::from_utf8_lossy(&chunk);
                // 백그라운드 개행 식별 버퍼의 꼬리에 덧붙입니다.
                line_buffer.push_str(&chunk_str);

                // 버퍼 안에 개행 기호(\n)가 식별되는 동안 문장을 반복적으로 토막 잘라냅니다.
                while let Some(pos) = line_buffer.find('\n') {
                    // 첫 개행 문자 앞쪽 위치까지 잘라내 문장으로 특정하고 트리밍합니다.
                    let line = line_buffer[..pos].trim().to_string();
                    // 사용이 마감된 줄 영역 및 개행 기호 자체를 버퍼에서 영구 제거(drain)합니다.
                    line_buffer.drain(..=pos);
                    
                    // 빈 빈칸 줄은 파싱 스킵하고 다음을 도모합니다.
                    if line.is_empty() { continue; }
                    // 정돈된 줄이 정상 규약의 NDJSON 객체 구조인지 직렬화 해독을 실행합니다.
                    if let Ok(j) = serde_json::from_str::<Value>(&line) {
                        // 입력(프리필) 소모 통계가 검출되었는지 확인 가사합니다.
                        if let Some(pt) = j.get("prompt_tokens").and_then(|v| v.as_i64()) {
                            // 토큰 계수 동기화
                            prompt_tokens = pt as i32;
                        }
                        // 출력(디코드) 생성 통계가 색출되었는지 점검합니다.
                        if let Some(ct) = j.get("completion_tokens").and_then(|v| v.as_i64()) {
                            // 토큰 계수 갱신
                            completion_tokens = ct as i32;
                        }
                        // 완성된 전체 대화 내역 목록 팩이 동봉 확인되었는지 조사합니다.
                        if let Some(hist) = j.get("history") {
                            // 배열 데이터 추출
                            if let Some(arr) = hist.as_array() {
                                // 사본 백업 저장
                                server_history = Some(arr.clone());
                            }
                        }
                        // 모델이 구동한 파이썬 도구의 반환 아웃풋이 묻어왔는지 점검합니다.
                        if let Some(tool_res) = j.get("tool_result") {
                            // 도구명 획득
                            if let Some(name) = tool_res.get("name").and_then(|v| v.as_str()) {
                                // 도구 반환값 획득
                                if let Some(result) = tool_res.get("result").and_then(|v| v.as_str()) {
                                    // 도구 결과 수령 통지를 격발해 보냅니다.
                                    let _ = net_tx.send(NetworkEvent::ToolResult {
                                        name: name.to_string(),
                                        result: result.to_string(),
                                    }).await;
                                }
                            }
                        }
                        // 에이전트 공정 지침 안내(guidance)가 흘러왔는지 진단합니다.
                        if let Some(guidance) = j.get("guidance").and_then(|v| v.as_str()) {
                            // 안내 이벤트 발송
                            let _ = net_tx.send(NetworkEvent::Guidance(guidance.to_string())).await;
                        }
                        
                        // 통신 규약 아래 실시간 메시지 객체(message) 정보가 탑재되었는지 확인합니다.
                        if let Some(msg) = j.get("message") {
                            // 도구 지시서(tool_calls) 데이터가 묻어 나왔는지 확인합니다.
                            if let Some(calls) = msg.get("tool_calls") {
                                // Null이 아니며 실제로 하나 이상의 지시가 들어 있는 리스트 구조인 것을 감시합니다.
                                if !calls.is_null() && calls.as_array().map_or(false, |a| !a.is_empty()) {
                                    // 사본 백업 기억
                                    tool_calls = Some(calls.clone());
                                    // 도구 지시 통지 이벤트 격발
                                    let _ = net_tx.send(NetworkEvent::ToolCall(calls.clone())).await;
                                }
                            }
                            // 텍스트 답변 내용물(content)이 공급되었는지 진단합니다.
                            if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                                // 비어있지 않다면
                                if !content.is_empty() {
                                    // 누적 전체 대답 저장소에 탑재
                                    full_response_content.push_str(content);
                                    // 청크 구문해석기에 청크를 물려 작업 계획/상태 블록과 가시 텍스트를 분류해 냅니다.
                                    let (events, visible_text) = parser.process_chunk(content);
                                    // 색출 도출된 각종 공정 갱신 이벤트들을 수신기 스레드로 축적 송출합니다.
                                    for ev in events {
                                        // 송출
                                        let _ = net_tx.send(ev).await;
                                    }
                                    // 정제 완료된 가시 텍스트 단편이 존재 시 Chunk 이벤트로 송출해 화면에 출력합니다.
                                    if !visible_text.is_empty() {
                                        // 텍스트 청크 송출
                                        let _ = net_tx.send(NetworkEvent::Chunk(visible_text)).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 통신 수집이 완료되었으므로 개행 해석 파서 내부에 정체 중인 잔여 문장 조각을 덤프 정제합니다.
            let (events, visible_text) = parser.flush();
            // 잔여 이벤트 송출
            for ev in events {
                let _ = net_tx.send(ev).await;
            }
            // 잔여 가시 텍스트도 마저 털어내 송출해 줍니다.
            if !visible_text.is_empty() {
                let _ = net_tx.send(NetworkEvent::Chunk(visible_text)).await;
            }

            // 서버 측에서 직렬화 역사 팩을 넘겨주었는지 감시하고 그렇지 않다면 로컬에서 대화 역사를 최종 짜맞춤합니다.
            let final_history = if let Some(sh) = server_history {
                // 서버 전송 팩 사용
                sh
            } else {
                // 기존 대화 역사에
                let mut local = history.clone();
                // 이번 생성 완료된 어시스턴트 대화 데이터를 JSON으로 조립해 병합합니다.
                let mut assistant_message = json!({
                    "role": "assistant",
                    "content": full_response_content.clone()
                });
                // 동작했던 도구 호출 내역이 있었다면 필드로 곁들여 결속합니다.
                if let Some(ref calls) = tool_calls {
                    // 결속 처리
                    assistant_message["tool_calls"] = calls.clone();
                }
                // 역사 탑재
                local.push(assistant_message);
                // 결과 인계
                local
            };

            // 통신 가동 루프 완수(Finished) 보고를 통계 정보들과 함께 송신하여 마무리합니다.
            let _ = net_tx.send(NetworkEvent::Finished {
                final_history,
                assistant_text: full_response_content,
                prompt_tokens,
                completion_tokens,
            }).await;
        });
    }

    // 비동기 통신 채널 스레드로부터 도착한 네트워크 변동 통보 이벤트(NetworkEvent)들을 메인 UI 메모리 상태판에 차례로 가산 보정하는 상태 갱신 함수입니다.
    pub fn handle_network_event(&mut self, event: NetworkEvent) {
        // 도착한 각 이벤트 유형별 구조 해석을 전개합니다.
        match event {
            // 가시 텍스트 토큰 청크를 접수했을 때
            NetworkEvent::Chunk(chunk) => {
                // 화면용 실시간 어시스턴트 버퍼에 가산 축적시킵니다.
                self.current_assistant_response.push_str(&chunk);
            }
            // 에이전트 작업 진행 정보판 목록이 업데이트되었을 때
            NetworkEvent::StatusUpdate(status) => {
                // TuiApp 상태 변수 동기화
                self.agent_status = status;
            }
            // 작업 계획 목록 상태가 수정 갱신되었을 때
            NetworkEvent::PlanUpdate(plan) => {
                // TuiApp 계획 목록 덮어쓰기 동기화
                self.agent_plan = plan;
            }
            // 도구 동작 안내 지침문이 도착했을 때
            NetworkEvent::ToolLog(log) => {
                // 실행 로그판의 후방에 로그 문자 추가
                self.tool_logs.push(log.clone());
                // 상세 타임라인판 지문에도 형식에 맞춰 로깅합니다.
                self.recursive_logs.push(format!("🪵 [로그] {}", log));
            }
            // 도구 호출 지시 JSON 정보가 탐색 보고되었을 때
            NetworkEvent::ToolCall(calls) => {
                // 호출 사양 리스트를 풀어서 식별 명칭과 매개변수를 문자열화하여 타임라인에 인쇄합니다.
                if let Some(arr) = calls.as_array() {
                    // 지시 묶음 순회
                    for tc in arr {
                        // 세부 함수 영역 분해
                        if let Some(func) = tc.get("function") {
                            // 실행 도구 실명 확보
                            let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("알 수 없음");
                            // 매개변수 확보
                            let args = func.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");
                            // 타임라인 등기
                            self.recursive_logs.push(format!("📞 [도구 호출] {}({})", name, args));
                        }
                    }
                }
            }
            // 도구 동작 결과가 수집 보고되었을 때
            NetworkEvent::ToolResult { name, result } => {
                // 타임라인에 도구 아웃풋 결과를 등기합니다.
                self.recursive_logs.push(format!("📥 [도구 결과] {} ➔ {}", name, result.trim()));
            }
            // 시스템 공정 실시간 안내문이 입수되었을 때
            NetworkEvent::Guidance(msg) => {
                // 안내문을 타임라인에 인쇄 보고합니다.
                self.recursive_logs.push(format!("🤖 [시스템 안내] {}", msg.trim()));
            }
            // 대화 라운드 전체 추론이 정상 완수된 통보를 수령했을 때
            NetworkEvent::Finished {
                final_history,
                assistant_text: _,
                prompt_tokens,
                completion_tokens,
            } => {
                // AI 대답 텍스트에 섞인 지저분한 특수 태그 문자들을 최종 정화 처리합니다.
                let clean = clean_tags(&self.current_assistant_response);
                // 정화 완료된 글귀가 부재하지 않다면 정식 어시스턴트 대화 히스토리에 편입 등기합니다.
                if !clean.is_empty() {
                    // 어시스턴트 역할로 등재
                    self.chat_history.push(("assistant".to_string(), clean));
                }
                // 동기화 완료된 전체 역사를 로컬 변수에 적용 갱신합니다.
                self.history_json_list = final_history;
                // 실시간 수집 임시 버퍼 소거
                self.current_assistant_response.clear();
                // 작업 상태 로딩 스위치 무효화 (대기 모드로 환원)
                self.is_loading = false;
                // 통계 카운터 동기화
                self.last_prompt_tokens = prompt_tokens;
                // 통계 카운터 동기화
                self.last_completion_tokens = completion_tokens;
                
                // 이번 턴의 총합 토큰 비용을 산출합니다.
                let total = prompt_tokens + completion_tokens;
                // 최대 예산(260k 토큰) 대비 소모 점유 비율을 계산합니다.
                let pct = (total as f64 / 262144.0) * 100.0;
                // 타임라인 로그에 최종 통계 내역을 로깅 인쇄합니다.
                self.recursive_logs.push(format!(
                    "🔋 [토큰 사용량] Prompt: {} | Completion: {} | Total: {} ({:.2}% of 260k context)",
                    prompt_tokens, completion_tokens, total, pct
                ));
            }
            // 비상 예외 오류 알림을 수신했을 때의 조치입니다.
            NetworkEvent::Error(err) => {
                // 로그판에 실패 표식 등기
                self.tool_logs.push(format!("❌ 오류 발생: {}", err));
                // 타임라인에 경보 기록 등록
                self.recursive_logs.push(format!("❌ 오류 발생: {}", err));
                // 대기 스위치 즉시 오프
                self.is_loading = false;
            }
        }
    }
}
