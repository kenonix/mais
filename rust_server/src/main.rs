// 프로젝트의 다른 파일 모듈(ffi.rs)을 ffi 이름의 서브 모듈로 선언합니다.
pub mod ffi;
// 프로젝트의 다른 파일 모듈(utils.rs)을 utils 이름의 서브 모듈로 선언합니다.
pub mod utils;
// 프로젝트의 다른 파일 모듈(tools.rs)을 tools 이름의 서브 모듈로 선언합니다.
pub mod tools;
// 프로젝트의 다른 파일 모듈(agentic.rs)을 agentic 이름의 서브 모듈로 선언합니다.
pub mod agentic;
// 프로젝트의 다른 파일 모듈(handlers.rs)을 handlers 이름의 서브 모듈로 선언합니다.
pub mod handlers;

// Axum 웹서버의 라우터 매핑 및 GET, POST 메소드 매핑용 도구를 임포트합니다.
use axum::{
    routing::{get, post},
    Router,
};
// HTTP 응답 헤더 값 검증처리를 위해 HeaderValue 타입을 가져옵니다.
use axum::http::HeaderValue;
// 웹 통신 리소스 도메인 간 공유 허용(CORS) 설정을 처리해 줄 레이어를 임포트합니다.
use tower_http::cors::CorsLayer;
// 여러 비동기 핸들러 간 스레드 통제가 보장되는 공유 참조 래퍼 Arc를 가져옵니다.
use std::sync::Arc;
// 에러 처리를 위한 anyhow의 Result 타입을 가져옵니다.
use anyhow::Result;

// ffi 모듈에서 C 라이브러리 엔진의 안전한 Wrapper 타입인 EngineWrapper를 참조합니다.
use crate::ffi::EngineWrapper;
// utils 모듈에서 시스템 초기 성격 및 행동 가이드 프롬프트를 긁어오는 함수를 가져옵니다.
use crate::utils::load_system_prompt;
// handlers 모듈에서 개별 API 엔드포인트별 비동기 바디 함수들을 대거 가져옵니다.
use crate::handlers::{
    handle_chat, handle_completions, handle_models, handle_root, handle_tags,
};

// 웹 서버 전체의 스레드 환경에서 안전하게 복사/공유되어 유통되는 글로벌 앱 상태 구조체 정의입니다.
#[derive(Clone)]
pub struct AppState {
    // 여러 스레드가 동시에 안전하게 접근할 수 있도록 포인팅 가드(Arc)로 감싼 추론 엔진 객체입니다.
    pub engine: Arc<EngineWrapper>,
    // 로드 완료되어 모든 세션 공통으로 쓰이는 시스템 프롬프트(성격 + 도구 가이드) 문자열입니다.
    pub system_prompt: String,
    // 클라이언트에 응답할 때 모델 구별용으로 사용될 모델 지칭 명칭입니다.
    pub model_name: String,
}

// Tokio 비동기 런타임을 기동시켜 main 함수가 비동기 비차단(async) 코드로 동작할 수 있게 선언합니다.
#[tokio::main]
// 서버가 구동 중 겪게 될 치명적인 시스템 IO 에러 등을 대비해 anyhow::Result 래퍼를 반환합니다.
async fn main() -> Result<()> {
    
    // GPU 가속 백엔드를 사용할지 여부를 보관하는 플래그 변수로 기본은 거짓(false)으로 정합니다.
    let mut use_gpu = false;
    // API 통신용으로 청취할 기본 포트 번호를 11434(Ollama 포트)로 배정합니다.
    let mut port = 11434;
    // 읽어들일 모델 파일의 기본 저장소 경로를 설정합니다.
    let mut model_path = "./models/multimodal_model.tflite".to_string();
    // 클라이언트에 표출해 줄 기본 모델 지칭명을 설정합니다.
    let mut model_name = "litert-lm:latest".to_string();
    
    // 사용자가 서버 구동 명령 시 기재한 터미널 커맨드 인자 리스트를 벡터 구조로 전부 수거합니다.
    let args: Vec<String> = std::env::args().collect();
    // 파싱을 시작할 인덱스 커서 값을 1로 설정합니다.
    let mut i = 1;
    // 전체 수거된 커맨드 인자 끝까지 순회를 진행합니다.
    while i < args.len() {
        // 커서 위치 인자 문자열 패턴 매칭을 전개합니다.
        match args[i].as_str() {
            // "--gpu" 인자를 감지했을 경우
            "--gpu" => {
                // GPU 가속 플래그를 참으로 세팅합니다.
                use_gpu = true;
            }
            // "--port" 포트번호 강제 지정 지시자를 만난 경우
            "--port" => {
                // 뒤따르는 인자 값이 범위 내에 올바르게 존재하는지 파악합니다.
                if i + 1 < args.len() {
                    // 다음 포트 값으로 인덱스를 전진합니다.
                    i += 1;
                    // 해당 문자열을 정수형 숫자로 올바르게 변싱하고, 성공 시 포트 번호를 오버라이딩합니다.
                    if let Ok(p) = args[i].parse() {
                        // 포트 번호 갱신
                        port = p;
                    }
                }
            }
            // "--model-name" 지명 인자가 감지된 경우
            "--model-name" => {
                // 대상 명칭 문자열 인자가 올바른 뒤이어 존재하는지 체크합니다.
                if i + 1 < args.len() {
                    // 뒤쪽 값으로 포인팅 커서를 이동시킵니다.
                    i += 1;
                    // 모델 표출 이름을 전달된 변수로 교체합니다.
                    model_name = args[i].clone();
                }
            }
            // 플래그 형식이 아닌 일반 텍스트 문장인 경우 모델 파일 경로로 취급합니다.
            arg => {
                // 대시(-) 기호로 시작하지 않는 단순 텍스트 경로인지 확인합니다.
                if !arg.starts_with('-') {
                    // 모델 탐색 경로 변수를 해당 경로로 재지정합니다.
                    model_path = arg.to_string();
                }
            }
        }
        // 다음 순서 인자를 기어링하기 위해 루프 오프셋을 증가시킵니다.
        i += 1;
    }
    
    // 시스템 성격을 모델에 맞물려 로드 시작함을 터미널에 보고합니다.
    println!("[시스템] 시스템 프롬프트 로드 중...");
    // 지침서 파일(soul.txt, tools.txt)들을 조합해 완성형 프롬프트를 획득합니다.
    let system_prompt = load_system_prompt();
    // 로딩에 안착한 지침 텍스트 내용을 한눈에 보기 편하게 출력합니다.
    println!("[시스템] 로드된 시스템 프롬프트:\n{}", system_prompt);
    
    // 모델 경로와 GPU 설정 상황을 프린트하며 모델 로드 공정을 안내합니다.
    println!("[시스템] 모델 로딩 중: {} (GPU: {})", model_path, use_gpu);
    // 엔진 래퍼 생성자를 기동하여 모델 파일을 적재하며, GPU 세팅 에러 발생 시의 폴백 처리를 연동합니다.
    let engine = match EngineWrapper::new(&model_path, use_gpu) {
        // 첫 번째 조건대로 엔진이 무사히 생성되었을 시 통과시킵니다.
        Ok(eng) => eng,
        // 오디오 장치 불일치 등의 연유로 기동 오류(Err)가 보고된 경우의 분기입니다.
        Err(e) => {
            // GPU 사용 모드로 구동하려다 실패했던 것인 경우
            if use_gpu {
                // 터미널 창에 GPU 로딩 실패 오류 상세를 노출하고 CPU 모드로 자동 전향함을 통보합니다.
                println!("[시스템] GPU 백엔드 로드 실패: {}. CPU 백엔드로 전환합니다...", e);
                // 강제 CPU 백엔드(use_gpu = false)로 설정하여 다시 엔진 생성자를 안전 격발합니다.
                EngineWrapper::new(&model_path, false)?
            } else {
                // CPU 모드로 구동하려다 실패한 완전 적재 불가인 경우 에러를 상위로 반환하고 정지합니다.
                return Err(e);
            }
        }
    };
    // 모델 로딩 완료 및 대기 준비 완료 상황을 공표합니다.
    println!("[시스템] 준비 완료!");
    
    // 여러 핸들러로 공유 유통시킬 AppState 구조체 데이터를 인스턴스화합니다.
    let state = AppState {
        // 엔진 소유권을 Arc 포인터 형태로 장착시킵니다.
        engine: Arc::new(engine),
        // 로드한 시스템 프롬프트를 보존합니다.
        system_prompt,
        // 지정된 모델명을 이양합니다.
        model_name,
    };
    
    // Axum 라우터를 선언하고 지원하는 모든 Ollama 및 OpenAI 규격 주소 라우팅 엔드포인트를 매핑 바인딩합니다.
    let app = Router::new()
        // Ollama 기본 루트
        .route("/", get(handle_root))
        // Ollama 모델 목록 tags 엔드포인트
        .route("/api/tags", get(handle_tags))
        // OpenAI 모델 조회 v1 엔드포인트
        .route("/v1/models", get(handle_models))
        // OpenAI 모델 조회 중첩 엔드포인트
        .route("/models", get(handle_models))
        // Ollama 대화형 채팅 chat 엔드포인트
        .route("/api/chat", post(handle_chat))
        // OpenAI v1 규격 비동기 completions 엔드포인트
        .route("/v1/chat/completions", post(handle_completions))
        // OpenAI completions 백업 엔드포인트
        .route("/chat/completions", post(handle_completions))
        // 다른 로컬 웹 UI 클라이언트에서 비동기로 접속할 수 있도록 모든 기점 허용(CORS) 미들웨어를 장착합니다.
        .layer(
            CorsLayer::new()
                // 오리진 와일드카드 허용 처리
                .allow_origin("*".parse::<HeaderValue>().unwrap())
                // 메소드 와일드카드 허용 처리
                .allow_methods(tower_http::cors::Any)
                // 헤더 와일드카드 허용 처리
                .allow_headers(tower_http::cors::Any),
        )
        // 개별 비동기 웹 요청 핸들러들이 공통 엔진 자원에 안전 교신하도록 글로벌 상태를 이식합니다.
        .with_state(state);
    
    // 수신 수락을 대기할 포트를 문자열 바인딩 주소로 변경 포맷팅합니다.
    let addr = format!("0.0.0.0:{}", port);
    // 지정된 소켓 주소로부터 입력을 받을 TCP 리스너를 비동기로 생성합니다.
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    // 정상 바인딩 완료되어 대기 국면에 안착했음을 터미널 창에 표기합니다.
    println!("[서버] {} 주소에서 대기 중...", addr);
    // Axum 비차단 웹서버 서빙 프로그램을 구동하여 HTTP 접속을 실시간 처리합니다.
    axum::serve(listener, app).await?;
    
    // 정상 서버 종료 시 빈 오케이 성공 상태를 반환합니다.
    Ok(())
}
