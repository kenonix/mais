// litert-lm-sys 크레이트(C 라이브러리 바인딩)를 sys라는 이름으로 가져옵니다.
use litert_lm_sys as sys;
// Rust 표준 라이브러리에서 C 호환 문자열 처리를 위해 CString을 가져옵니다.
use std::ffi::CString;
// Rust 표준 라이브러리에서 원시 포인터 처리를 위해 ptr 모듈을 가져옵니다.
use std::ptr;
// anyhow 크레이트에서 에러 처리를 위한 Result와 anyhow 매크로를 가져옵니다.
use anyhow::{Result, anyhow};

// LiteRT-LM C API의 대화 관련 추가 인자를 나타내는 불투명한(opaque) 열거형 타입 정의입니다.
// 이 타입은 C 내부에서 관리되므로 Rust 쪽에서는 실제 필드가 없는 빈 열거형으로 정의합니다.
pub enum LiteRtLmConversationOptionalArgs {}

// C 언어로 작성된 외부 라이브러리 함수들을 Rust에서 호출하기 위한 외부(FFI) 연결 블록입니다.
extern "C" {
    // LiteRT-LM 대화 옵션 설정을 위한 객체를 생성하고 포인터를 반환하는 C 함수입니다.
    pub fn litert_lm_conversation_optional_args_create() -> *mut LiteRtLmConversationOptionalArgs;
    
    // 생성된 대화 옵션 객체를 메모리에서 해제하는 C 함수입니다.
    pub fn litert_lm_conversation_optional_args_delete(
        optional_args: *mut LiteRtLmConversationOptionalArgs,
    );
    
    // 비주얼(이미지 등) 관련 토큰 예산을 설정하는 C 함수입니다.
    pub fn litert_lm_conversation_optional_args_set_visual_token_budget(
        optional_args: *mut LiteRtLmConversationOptionalArgs,
        visual_token_budget: std::os::raw::c_int,
    );
    
    // 최대 출력 토큰 길이를 제한하는 C 함수입니다.
    pub fn litert_lm_conversation_optional_args_set_max_output_tokens(
        optional_args: *mut LiteRtLmConversationOptionalArgs,
        max_output_tokens: std::os::raw::c_int,
    );
    
    // 스트리밍 방식으로 대화 메시지를 엔진으로 전송하는 핵심 FFI C 함수입니다.
    pub fn litert_lm_conversation_send_message_stream(
        conversation: *mut sys::LiteRtLmConversation,
        message_json: *const std::os::raw::c_char,
        extra_context: *const std::os::raw::c_char,
        optional_args: *const LiteRtLmConversationOptionalArgs,
        callback: sys::LiteRtLmStreamCallback,
        callback_data: *mut std::os::raw::c_void,
    ) -> std::os::raw::c_int;
    
    // 엔진의 설정 값 중에서 생성 가능한 최대 토큰 개수를 설정하는 C 함수입니다.
    pub fn litert_lm_engine_settings_set_max_num_tokens(
        settings: *mut sys::LiteRtLmEngineSettings,
        max_num_tokens: std::os::raw::c_int,
    );
    
    // 대화 객체로부터 벤치마크(토큰 수, 성능 등) 정보 객체를 가져오는 C 함수입니다.
    pub fn litert_lm_conversation_get_benchmark_info(
        conversation: *mut sys::LiteRtLmConversation,
    ) -> *mut sys::LiteRtLmBenchmarkInfo;
    
    // 사용이 끝난 벤치마크 정보 객체의 메모리를 해제하는 C 함수입니다.
    pub fn litert_lm_benchmark_info_delete(
        benchmark_info: *mut sys::LiteRtLmBenchmarkInfo,
    );
    
    // 벤치마크 정보 객체로부터 프리필(컨텍스트 로드) 단계의 턴 수를 조회하는 C 함수입니다.
    pub fn litert_lm_benchmark_info_get_num_prefill_turns(
        benchmark_info: *const sys::LiteRtLmBenchmarkInfo,
    ) -> std::os::raw::c_int;
    
    // 벤치마크 정보 객체로부터 디코드(텍스트 생성) 단계의 턴 수를 조회하는 C 함수입니다.
    pub fn litert_lm_benchmark_info_get_num_decode_turns(
        benchmark_info: *const sys::LiteRtLmBenchmarkInfo,
    ) -> std::os::raw::c_int;
    
    // 지정한 인덱스에 해당하는 프리필 단계의 토큰 개수를 반환하는 C 함수입니다.
    pub fn litert_lm_benchmark_info_get_prefill_token_count_at(
        benchmark_info: *const sys::LiteRtLmBenchmarkInfo,
        index: std::os::raw::c_int,
    ) -> std::os::raw::c_int;
    
    // 지정한 인덱스에 해당하는 디코드 단계의 토큰 개수를 반환하는 C 함수입니다.
    pub fn litert_lm_benchmark_info_get_decode_token_count_at(
        benchmark_info: *const sys::LiteRtLmBenchmarkInfo,
        index: std::os::raw::c_int,
    ) -> std::os::raw::c_int;
    
    // 엔진 설정 빌더에서 성능 측정을 위한 벤치마크 모드를 활성화하는 C 함수입니다.
    pub fn litert_lm_engine_settings_enable_benchmark(
        settings: *mut sys::LiteRtLmEngineSettings,
    );
}

// C 언어의 Raw 포인터를 안전하게 래핑하여 Rust에서 안전하게 사용하도록 돕는 EngineWrapper 구조체입니다.
pub struct EngineWrapper {
    // NonNull은 널(null)이 아님이 보장되는 원시 포인터를 가리키며 공변성(covariance)을 가집니다.
    pub ptr: ptr::NonNull<sys::LiteRtLmEngine>,
}

// Raw 포인터는 기본적으로 스레드 간 전송(Send)이 불가능하므로, 안전함을 컴파일러에게 명시합니다.
unsafe impl Send for EngineWrapper {}
// Raw 포인터는 기본적으로 여러 스레드에서 공유(Sync)가 불가능하므로, 안전함을 컴파일러에게 명시합니다.
unsafe impl Sync for EngineWrapper {}

// EngineWrapper 구조체가 스코프를 벗어나 소멸할 때 자원을 해제하기 위한 Drop 트레이트 구현입니다.
impl Drop for EngineWrapper {
    // 객체가 drop될 때 실행되는 함수입니다.
    fn drop(&mut self) {
        // C 라이브러리의 자원을 해제하는 것이므로 unsafe 블록을 사용합니다.
        unsafe {
            // C API 함수를 호출하여 LiteRT LM 엔진 객체를 파괴하고 메모리를 비웁니다.
            sys::litert_lm_engine_delete(self.ptr.as_ptr());
        }
    }
}

// EngineWrapper 구조체에 새로운 메서드를 추가하는 impl 구현체 블록입니다.
impl EngineWrapper {
    // 모델 경로와 GPU 가속 활성화 여부를 전달받아 새로운 엔진 인스턴스를 생성하는 생성자입니다.
    pub fn new(model_path: &str, use_gpu: bool) -> Result<Self> {
        // Rust의 문자열(&str)을 C 호환 널 종료 문자열(CString)로 변환합니다. 에러 발생 시 반환합니다.
        let model_path_cstr = CString::new(model_path)?;
        // GPU를 사용할 경우 "gpu" 백엔드 문자열을 선택하고, 그렇지 않으면 "cpu"를 사용합니다.
        let backend = if use_gpu { "gpu" } else { "cpu" };
        // 선택된 백엔드 이름 문자열을 CString으로 변환합니다.
        let backend_cstr = CString::new(backend)?;
        // 비전(이미지 처리)용 백엔드 이름도 동일하게 CString으로 생성합니다.
        let vision_backend_cstr = CString::new(backend)?;
        // 오디오 모델은 GPU 연동 시 에러를 유발하므로 항상 "cpu"로 고정하여 CString을 생성합니다.
        let audio_backend_cstr = CString::new("cpu")?;

        // C API를 호출하여 설정(settings) 객체를 생성합니다. 이 과정은 Raw 포인터를 다루므로 unsafe합니다.
        let settings = unsafe {
            sys::litert_lm_engine_settings_create(
                // 모델 파일 경로의 원시 포인터 주소를 전달합니다.
                model_path_cstr.as_ptr(),
                // 메인 LLM 텍스트 백엔드 명칭 포인터를 전달합니다.
                backend_cstr.as_ptr(),
                // 비전 인코더 백엔드 명칭 포인터를 전달합니다.
                vision_backend_cstr.as_ptr(),
                // 오디오 인코더 백엔드 명칭 포인터를 전달합니다.
                audio_backend_cstr.as_ptr(),
            )
        };
        // 설정 객체가 널 포인터이면 생성에 실패한 것이므로 에러를 반환합니다.
        if settings.is_null() {
            // anyhow 라이브러리를 통해 이쁘게 정리된 에러를 전달합니다.
            return Err(anyhow!("Failed to create engine settings"));
        }
        
        // 설정 객체가 정상적으로 생성되었으므로 부가적인 옵션들을 세팅해줍니다.
        unsafe {
            // 엔진이 컨텍스트 내에서 가질 수 있는 최대 토큰 처리량을 매우 큰 값(262144)으로 늘려줍니다.
            litert_lm_engine_settings_set_max_num_tokens(settings, 262144);
            // 각 추론 턴별 토큰 생성 속도와 성능 벤치마크 데이터를 수집할 수 있도록 활성화합니다.
            litert_lm_engine_settings_enable_benchmark(settings);
        }
        
        // 설정을 바탕으로 실제 동작할 런타임 엔진(engine) 인스턴스를 빌드합니다.
        let engine_ptr = unsafe { sys::litert_lm_engine_create(settings) };
        // 엔진이 생성되었으므로 더 이상 필요하지 않은 템플릿용 설정 객체의 메모리를 파괴해줍니다.
        unsafe { sys::litert_lm_engine_settings_delete(settings) };
        
        // 최종 생성된 엔진 포인터가 널(Null) 포인터인지 검사하고 NonNull 래퍼로 감싸 줍니다.
        let ptr = ptr::NonNull::new(engine_ptr).ok_or_else(|| anyhow!("Failed to create engine"))?;
        // 최종 감싸진 EngineWrapper의 Self 구조체 형태로 성공 결과값(Ok)을 반환합니다.
        Ok(Self { ptr })
    }
}
