// serde 크레이트에서 구조체를 직렬화(Serialize) 및 역직렬화(Deserialize)할 수 있는 매크로를 가져옵니다.
use serde::{Deserialize, Serialize};
// serde_json 크레이트에서 JSON 객체를 동적으로 생성하기 위한 json 매크로와 임의 값 처리를 위한 Value 타입을 임포트합니다.
use serde_json::{json, Value};
// 파일 시스템 읽기/쓰기를 위해 표준 라이브러리 fs 모듈을 가져옵니다.
use std::fs;
// 파일 및 디렉토리의 경로 조작을 위해 Path 및 PathBuf 구조체를 임포트합니다.
use std::path::{Path, PathBuf};

// AI 에이전트의 성격 정보(soul.txt)가 설정되지 않았을 때 fallback용으로 동작할 기본 영혼(프롬프트) 상수입니다.
pub const DEFAULT_SOUL: &str = r#"당신의 이름은 AI입니다.
한국어만 사용하며, 친절하고 명확하게 답변합니다.
사용자에게 보이는 답변은 자연스러운 평문을 우선합니다.
수학적 그래프 시각화가 필요할 경우 수식을 평문으로 설명하고, 필요한 경우 그래프 도구를 사용하세요."#;

// LLM 튜닝 하이퍼파라미터 및 시스템 프롬프트 설정을 캡슐화한 설정 구조체입니다.
#[derive(Clone, Serialize, Deserialize)]
pub struct ChatSettings {
    // LLM 생성 무작위성을 제어하는 온도 값입니다.
    pub temperature: f32,
    // 누적 확률 한계치를 설정하는 top_p 값입니다.
    pub top_p: f32,
    // 상위 후보군 개수를 한정하는 top_k 값입니다.
    pub top_k: u32,
    // 최대 토큰 출력량을 나타내는 한계치 값입니다.
    pub max_tokens: u32,
    // 모델의 근간 성격을 주입하는 통합 시스템 지침 문자열입니다.
    pub system_prompt: String,
}

// ChatSettings 구조체의 기본 초깃값 매핑을 구성해 줍니다.
impl Default for ChatSettings {
    // 기본 생성자 매핑 함수입니다.
    fn default() -> Self {
        // 기본 권장 사양 설정값을 지정하여 빌드합니다.
        Self {
            // 온도는 기본 0.7로 책정합니다.
            temperature: 0.7,
            // top_p는 기본 0.95로 지정합니다.
            top_p: 0.95,
            // top_k는 기본 40으로 조율합니다.
            top_k: 40,
            // 토큰 예산은 최대 262,144개로 제한합니다.
            max_tokens: 262144,
            // 시스템 프롬프트는 파일들로부터 로드해 세팅합니다.
            system_prompt: load_system_prompt(),
        }
    }
}

// soul.txt와 tools.txt 파일의 본문을 읽어와 단일 통합 시스템 프롬프트 문자열로 병합 조립하는 함수입니다.
pub fn load_system_prompt() -> String {
    // soul.txt 파일의 물리적 경로 후보군 목록을 정의합니다.
    let soul = ["../soul.txt", "soul.txt"]
        .iter() // 후보군 목록을 차례대로 스캔합니다.
        .filter_map(|p| fs::read_to_string(p).ok()) // 정상적으로 열려 해독 가능한 파일의 데이터만 걸러냅니다.
        .find(|c| !c.trim().is_empty()) // 파일 본문 내용이 공백이 아닌 유효한 최초 대상을 색출합니다.
        .unwrap_or_else(|| DEFAULT_SOUL.to_string()); // 어떠한 파일도 부재하거나 비었다면 기본 영혼 상수로 대체 복원합니다.

    // tools.txt 파일의 물리적 경로 후보군 목록을 설정합니다.
    let tools = ["../tools.txt", "tools.txt"]
        .iter() // 후보군 목록을 스캔합니다.
        .filter_map(|p| fs::read_to_string(p).ok()) // 열려 읽을 수 있는 파일 내용만 발굴합니다.
        .find(|c| !c.trim().is_empty()) // 실체가 들어 있는 파일 데이터만 최종 선택합니다.
        .unwrap_or_default(); // 부재 시에는 빈 문자열("")로 fallback 처리합니다.

    // 도구 스키마(tools) 정의가 존재하지 않는지 분기 확인합니다.
    if tools.is_empty() {
        // 영혼 프롬프트만 끝 공백을 잘라 반환합니다.
        soul.trim().to_string()
    } else {
        // 영혼 프롬프트 하단에 도구 설명 가이드 명세를 융합하여 반환합니다.
        format!("{}\n\n{}", soul.trim(), tools.trim())
    }
}

// 로컬 설정 파일(config.json)의 기록들을 분석하여 유동적으로 ChatSettings 구조체를 복원하는 함수입니다.
pub fn load_settings() -> ChatSettings {
    // 우선 기본적인 기본값(Default) 설정 팩으로 틀을 잡습니다.
    let mut settings = ChatSettings::default();
    // 설정 정보가 들어 있을 법한 경로 후보군 리스트를 정의합니다.
    let config_paths = ["../config.json", "config.json"];
    // 각 경로 후보를 순회 탐색합니다.
    for path in &config_paths {
        // 설정 파일 로드를 시도합니다.
        if let Ok(content) = fs::read_to_string(path) {
            // 로드 완료된 텍스트가 올바른 JSON 파싱을 거칠 수 있는지 검사합니다.
            if let Ok(j) = serde_json::from_str::<Value>(&content) {
                // temperature 설정이 존재 시 해당 값으로 덮어씁니다.
                if let Some(t) = j.get("temperature").and_then(|v| v.as_f64()) {
                    // f32 타입으로 형변환하여 갱신
                    settings.temperature = t as f32;
                }
                // top_p 설정이 확인되는 경우 갱신을 수행합니다.
                if let Some(p) = j.get("top_p").and_then(|v| v.as_f64()) {
                    // 갱신 반영
                    settings.top_p = p as f32;
                }
                // top_k 설정이 유효하게 주입되었는지 진단합니다.
                if let Some(k) = j.get("top_k").and_then(|v| v.as_u64()) {
                    // 적용
                    settings.top_k = k as u32;
                }
                // max_output_tokens 한계 설정 값이 들어 있는지 체크합니다.
                if let Some(m) = j.get("max_output_tokens").and_then(|v| v.as_u64()) {
                    // 적용
                    settings.max_tokens = m as u32;
                }
                // 설정값 갱신이 완료되었으므로 후방 스캔 순회를 중단하고 이탈합니다.
                break;
            }
        }
    }
    // 완성된 최종 설정 데이터를 반환합니다.
    settings
}

// 변경된 하이퍼파라미터 및 시스템 프롬프트를 로컬 디스크 파일(config.json, soul.txt)에 영구 저장하는 동기화 함수입니다.
pub fn save_settings(settings: &ChatSettings) {
    // 변경된 성격 지침서 본문을 soul.txt 파일에 즉각 기재 반영합니다.
    let _ = fs::write("soul.txt", &settings.system_prompt);
    // 각각의 튜닝 변수 데이터를 맵 형태의 JSON 밸류 객체로 역조립합니다.
    let config_j = json!({
        "temperature": settings.temperature,
        "top_p": settings.top_p,
        "top_k": settings.top_k,
        "max_output_tokens": settings.max_tokens
    });
    // 직관적인 사람이 읽을 수 있는 포맷(pretty format) 문자열로 가공해 냅니다.
    if let Ok(formatted) = serde_json::to_string_pretty(&config_j) {
        // config.json 파일에 기록 갱신을 단행합니다.
        let _ = fs::write("config.json", formatted);
    }
}

// 틸데 물결 문자(~)가 서두에 묻은 파일 상대 경로를 물리 환경 홈 디렉토리의 전체 절대 경로로 환원 확장해 주는 함수입니다.
pub fn expand_path(path_str: &str) -> String {
    // 앞뒤에 섞인 지저분한 여백을 제거합니다.
    let trimmed = path_str.trim();
    // 홈 디렉토리의 치환 태그 문자(~)로 첫 스타트를 끊었는지 조건 비교합니다.
    if trimmed.starts_with('~') {
        // OS 환경 변수에서 사용자의 물리 HOME 주소를 찾아냅니다.
        if let Some(home) = std::env::var_os("HOME") {
            // 홈 경로를 탑재한 PathBuf 객체를 만듭니다.
            let mut p = PathBuf::from(home);
            // 틸데 기호 이외에 후방 세부 주소가 추가 기입되어 있는지 점검합니다.
            if trimmed.len() > 1 {
                // 슬라이싱하여 (~/) 뒤의 세부 경로를 PathBuf 객체에 연동 적재시킵니다.
                p.push(&trimmed[2..]);
            }
            // 절대 경로 텍스트 문자열 형태로 최종 반환합니다.
            return p.to_string_lossy().to_string();
        }
    }
    // 치환 태그 대상이 아닌 일반 경로인 경우 단순 보정 후 텍스트 반환합니다.
    Path::new(trimmed).to_string_lossy().to_string()
}
