// serde_json 크레이트에서 임의의 복잡한 JSON 구조를 다루는 Value 타입을 임포트합니다.
use serde_json::Value;

// 서버(rust_server)로부터 비동기식 채널(mpsc)을 타고 유입되는 모든 개별 상태 변동 신호들을 보존하는 전파용 열거형입니다.
#[allow(dead_code)]
pub enum NetworkEvent {
    // LLM 모델이 실시간으로 생성해 낸 한 조각의 텍스트 토큰입니다.
    Chunk(String),
    // 현재 동작 중인 전체 에이전트의 개별 진행 공정 상태(Status) 목록 업데이트 알림입니다.
    StatusUpdate(Vec<String>),
    // 모델이 자율 수립한 단계별 추론 작업 계획(Plan) 목록의 최신 진행 상태 업데이트 알림입니다.
    PlanUpdate(Vec<String>),
    // 도구 생성 및 호출 중계 내역을 나타내는 텍스트 로그입니다.
    ToolLog(String),
    // 모델이 특정 도구를 사용하겠다고 뱉은 원시 JSON 호출 지시 배열 구조 데이터입니다.
    ToolCall(Value),
    // 기동 완료된 도구의 실제 파이썬 표준출력 결과 보고 데이터입니다.
    ToolResult {
        // 호출되어 수행된 도구의 명칭
        name: String,
        // 해당 도구가 최종 리턴한 실행 본안 텍스트
        result: String,
    },
    // 현재 시스템 공정 상 진행 안내 멘트(Guidance) 알림 정보입니다.
    Guidance(String),
    // 모든 연쇄 동작 체인이 완결되어 역사 목록과 토큰 소비 통계가 산출된 최종 마감 이벤트입니다.
    Finished {
        // 연동 교정이 완료된 서버 측 최종 대화 역사 배열 리스트
        final_history: Vec<Value>,
        // 최종 텍스트 내용 전체 취합본
        assistant_text: String,
        // 질문 분석에 소비된 입력 토큰 총합
        prompt_tokens: i32,
        // 답변 생성에 소모된 출력 토큰 총합
        completion_tokens: i32,
    },
    // 통신 오류, 스트림 중단, 서버 이상 등의 긴급 에러 발생 시 전파되는 예외 경보 이벤트입니다.
    Error(String),
}

// 실시간 수신되는 청크 파이프라인 데이터에서 개행 문자(\n) 단위로 문장을 복조하고, 특정 텍스트 블록(작업 상태/작업 계획)을 선별 추적하는 스트림 구문 해석기 구조체입니다.
pub struct StreamParser {
    // 현재 파싱 포인터가 작업 상태 기록 구역("[작업 상태]") 내부에 위치해 있는지 관리하는 불리언 플래그입니다.
    pub in_status_block: bool,
    // 현재 파싱 포인터가 작업 계획 기록 구역("[작업 계획]") 내에 있는지 확인하는 체크용 플래그입니다.
    pub in_plan_block: bool,
    // 현재까지 누적 탐색해 확보한 작업 상태 목록 문자열들을 동적으로 담아 두는 벡터입니다.
    pub accumulated_status: Vec<String>,
    // 현재 라운드까지 축적해 기록한 작업 계획 단계 상세 텍스트 목록을 담아두는 벡터입니다.
    pub accumulated_plan: Vec<String>,
    // 개행 문자(\n)가 도달하기 전까지 청크 문자들을 임시로 덧붙여 적재해두는 캐싱용 문자열 버퍼입니다.
    pub line_buffer: String,
}

// StreamParser 구조체 내의 연산 기능 함수들을 구현해 올립니다.
impl StreamParser {
    // 구문 파서의 상태 판을 초기 미사용 상태로 깔끔하게 구성해 생성하는 생성자입니다.
    pub fn new() -> Self {
        // 멤버 필드들을 초기 상태로 세팅하여 인스턴스화합니다.
        Self {
            // 상태 블록 미진입 상태 지정
            in_status_block: false,
            // 계획 블록 미진입 상태 지정
            in_plan_block: false,
            // 빈 리스트로 초기 배정
            accumulated_status: Vec::new(),
            // 빈 리스트로 초기 할당
            accumulated_plan: Vec::new(),
            // 빈 임시 캐시 확보
            line_buffer: String::new(),
        }
    }

    // 소켓 스트림으로부터 유입된 원시 청크 단편 문자열 조각을 접수해 내부에 수납하고, 완결된 문장 단위 이벤트들을 일괄 정합 분석해 반환하는 함수입니다.
    pub fn process_chunk(&mut self, chunk: &str) -> (Vec<NetworkEvent>, String) {
        // 스캔 결과 색출해 낸 네트워크 수신 이벤트 목록을 담을 지역 임시 버퍼입니다.
        let mut events = Vec::new();
        // 사용자 화면 UI의 챗 히스토리 판에 노출 중계해 줄 일반 순수 가독 텍스트 누적 버퍼입니다.
        let mut visible_text = String::new();

        // 입력된 청크 단편의 문자들을 하나 단위로 정밀 순회 분석합니다.
        for c in chunk.chars() {
            // 개행 문자(\n) 분기점을 마주쳤는지 점검합니다.
            if c == '\n' {
                // 완성된 한 줄의 문장을 임시 캐시 버퍼로부터 소유권을 취득하여 분양해 옵니다.
                let line = std::mem::take(&mut self.line_buffer);
                // 한 줄 단위 문장 정화기 함수를 구동시켜 이벤트 및 노출 텍스트를 파싱합니다.
                let (ev_opt, txt_opt) = self.process_finished_line(&line);
                // 네트워크로 전파할 고유 이벤트가 매핑되었다면 리스트에 적재합니다.
                if let Some(ev) = ev_opt {
                    // 이벤트 추가
                    events.push(ev);
                }
                // 사용자 텍스트가 걸러져 복원되었다면 노출 버퍼에 가산합니다.
                if let Some(txt) = txt_opt {
                    // 가시 텍스트 추가
                    visible_text.push_str(&txt);
                }
            } else {
                // 개행 문자가 아직 도달하지 않았다면 임시 라인 버퍼의 후방에 텍스트 문자를 계속 적재합니다.
                self.line_buffer.push(c);
            }
        }

        // 도출해 낸 통신 이벤트 묶음과 사용자 가시 텍스트 문자열의 튜플 구조를 반환합니다.
        (events, visible_text)
    }

    // 완전히 끊어 구분해 낸 한 줄 단위의 완성 텍스트를 인가받아, 작업 상태/계획 블록 여부 및 가이드 로그를 판별해 내는 파싱 함수입니다.
    pub fn process_finished_line(&mut self, line: &str) -> (Option<NetworkEvent>, Option<String>) {
        // 문장 좌우측의 지저분한 여백과 개행 잔여물을 트리밍 정돈합니다.
        let trimmed = line.trim();
        
        // 작업 상태 지시문 영역("[작업 상태]")의 서막을 가리키는 고정 지시어인지 검사합니다.
        if trimmed == "작업 상태" {
            // 상태 블록 활성화 플래그 참 처리
            self.in_status_block = true;
            // 계획 블록 비활성화 처리
            self.in_plan_block = false;
            // 기존 누적 저장해 둔 임시 목록 비우기
            self.accumulated_status.clear();
            // 이벤트 통보 없이 조기 리턴
            return (None, None);
        }
        
        // 작업 계획 지시문 영역("[작업 계획]")의 개막을 알리는 지시어 패턴인지 분기합니다.
        if trimmed == "작업 계획" {
            // 상태 블록 활성 통제 오프
            self.in_status_block = false;
            // 계획 블록 활성화 온
            self.in_plan_block = true;
            // 누적 계획 내역 클리어
            self.accumulated_plan.clear();
            // 이벤트 통보 생략 리턴
            return (None, None);
        }

        // 현재 스캔이 상태 구역 안에서 전개되는 중일 때의 공정입니다.
        if self.in_status_block {
            // 줄 간의 공백 문자라면 가볍게 무시하고 통과시킵니다.
            if trimmed.is_empty() {
                // 조기 이탈
                return (None, None);
            }
            // "키 : 값" 구조를 보존한 일반 상태 매핑 라인인 경우
            if trimmed.contains(':') {
                // 누적 리스트에 문장을 등기합니다.
                self.accumulated_status.push(trimmed.to_string());
                // 최신 상태 리스트를 동봉한 StatusUpdate 이벤트를 즉각 격발 보고합니다.
                return (Some(NetworkEvent::StatusUpdate(self.accumulated_status.clone())), None);
            } else {
                // "키:값" 형식이 아닌 텍스트가 흘러들어왔다면 상태 구역의 종결로 보고 플래그를 회수합니다.
                self.in_status_block = false;
                // 문장에 혹시 묻어 있을 수 있는 태그를 정화합니다.
                let clean = clean_tags(line);
                // 정화된 문자열이 빈 상태가 아니면 개행과 연동해 사용자 화면에 흘려보냅니다.
                return if clean.is_empty() { (None, None) } else { (None, Some(clean + "\n")) };
            }
        }

        // 현재 파싱 처리가 계획 구역 내부에서 매핑되는 상태일 때입니다.
        if self.in_plan_block {
            // 공백 라인을 만났다면 구역이 종결된 것으로 감지하고 이탈합니다.
            if trimmed.is_empty() {
                // 오프 처리
                self.in_plan_block = false;
                // 리턴
                return (None, None);
            }
            // "상태:" 혹은 "단계:" 처럼 계획 구조를 서술하는 유효 패턴 지문인지 진단합니다.
            if trimmed.contains("상태:") || trimmed.contains("단계:") {
                // 누적 계획 데이터 벡터에 탑재합니다.
                self.accumulated_plan.push(trimmed.to_string());
                // 갱신된 리스트를 PlanUpdate 이벤트를 타고 클라이언트로 송출합니다.
                return (Some(NetworkEvent::PlanUpdate(self.accumulated_plan.clone())), None);
            } else {
                // 예상치 못한 평이한 텍스트 줄을 마주한 경우 계획 구역을 퇴거합니다.
                self.in_plan_block = false;
                // 태그 정제 수행
                let clean = clean_tags(line);
                // 가시 텍스트 반환 조율
                return if clean.is_empty() { (None, None) } else { (None, Some(clean + "\n")) };
            }
        }

        // 모델이 자율 에이전트 도구 구동에 연관된 제어용 통지 텍스트를 인쇄했는지 스캔합니다.
        if trimmed.starts_with("도구를 만듭니다. 이름:") 
            || trimmed.starts_with("도구를 사용합니다. 이름:")
            || trimmed.starts_with("[시스템 자동 지시]") 
        {
            // 실행 모니터링 로그(ToolLog) 이벤트를 포장해 클라이언트로 쏩니다.
            return (Some(NetworkEvent::ToolLog(trimmed.to_string())), None);
        }

        // 특수한 지시문 구간이 아닌 일반적인 대화 생성 영역의 문자열인 경우 태그를 정화합니다.
        let clean = clean_tags(line);
        // 비어있는 무의미 문장이 아니면 정상적인 개행 텍스트로 사용자 스크린에 중계 방출합니다.
        if clean.is_empty() {
            // 반환 없음
            (None, None)
        } else {
            // 중계 처리
            (None, Some(clean + "\n"))
        }
    }

    // 소켓 스트림 로딩 완수 시 라인 캐시 버퍼에 어설프게 남아 있는 잔여 잔당 문자들을 최종적으로 쏟아내 정합 가공하는 함수입니다.
    pub fn flush(&mut self) -> (Vec<NetworkEvent>, String) {
        // 캐시 버퍼가 완벽히 비어 있는 깔끔한 상태라면 후속 처리를 생략하고 조기 종결합니다.
        if self.line_buffer.is_empty() {
            return (Vec::new(), String::new());
        }
        // 캐싱 문자열의 소유권을 완전 회수하여 가공 대상으로 삼습니다.
        let line = std::mem::take(&mut self.line_buffer);
        // 최종 라인 처리를 기동합니다.
        let (ev_opt, txt_opt) = self.process_finished_line(&line);
        // 리턴 팩 준비
        let mut events = Vec::new();
        let mut visible_text = String::new();
        // 잔여 이벤트 취합
        if let Some(ev) = ev_opt {
            // 이벤트 탑재
            events.push(ev);
        }
        // 잔여 텍스트 조율
        if let Some(txt) = txt_opt {
            // 텍스트 탑재
            visible_text.push_str(&txt);
        }
        // 최종 튜플 반환
        (events, visible_text)
    }
}

// AI 에이전트 생성 본문 내에 노출되거나 섞여 들어간 특수 메타 태그 문자열들을 제거해 순수한 본문 글귀만 빼내는 전처리 유틸리티 함수입니다.
pub fn clean_tags(line: &str) -> String {
    // 자가 사용자 입력 개시 태그([USER_INPUT]), 미완결 태그 및 닫는 마감 지시자 기호들을 전부 찾아 공백 문자로 치환 및 절단 정제합니다.
    line.replace("[USER_INPUT]", "")
        .replace("[USER_INPUT", "")
        .replace("[END_INPUT]", "")
        .replace("[END_INPUT", "")
        .trim_end_matches(']') // 지시자 뒤에 남을 수 있는 잔여 대괄호 문자도 슬라이싱 소거합니다.
        .to_string() // 문자열로 반환
}
