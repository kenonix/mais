# LiteRT-LM Multimodal API Server & Agentic Tool Synthesizer

Google의 **LiteRT-LM** (구 TensorFlow Lite-LM) C/C++ 엔진을 활용하여 로컬에서 멀티모달 LLM을 실행하고, AI가 스스로 도구를 개발 및 수정하여 문제를 해결하는 **자율적 동적 도구 빌드체인(Agentic Tool Synthesis)**이 내장된 고성능 API 서버와 이를 연동하는 Rust CLI 클라이언트 프로젝트입니다.

> [!NOTE]
> 기존 C++ 서버(`main.cpp`)의 모든 기능(멀티모달, 자율 에이전트 루프, Ollama/OpenAI 호환성)이 **Rust (`rust_server`)**로 성공적으로 마이그레이션되었습니다.

---

## 🚀 주요 기능

### 1. 🦀 Rust 기반 고성능 API 서버 (`rust_server`)
*   **Axum 프레임워크**: 고성능 비동기 웹 프레임워크인 Axum을 기반으로 설계되어 안정적이고 빠른 API 엔드포인트 응답 속도를 보장합니다.
*   **안정적인 멀티스레드 에이전트 루프**: LiteRT-LM C++ 엔진의 블로킹 추론부를 별도의 OS 스레드로 안전하게 격리하여 서버의 이벤트 루프가 멈추지 않도록 구현되었습니다.
*   **FFI/ABI 정합성 유지**: 최신 LiteRT-LM C API의 `optional_args` 6개 인자 구조 규격을 직접 재선언하여 크레이트 버전 불일치로 인한 링킹/런타임 에러를 방지했습니다.
*   **Visual Token Budget 자동 관리**: 비전 모델 로드 시 발생할 수 있는 토큰 할당 오류를 방지하기 위해 `optional_args` 구조체를 통해 최적의 Visual Token Budget(기본: 1024)을 동적 주입합니다.

### 2. 🛠️ AI 자율적 동적 도구 생성 및 활용 (Agentic Tool Synthesis)
*   **메타 도구 (`create_or_update_tool`)**: AI가 자신이 해결할 수 없는 복잡한 작업(로컬 파일 조회, 시스템 정보 검색, 외부 API 연동 등)을 마주했을 때, 스스로 적합한 파이썬 스크립트 도구를 작성하고 시스템에 실시간으로 등록합니다.
*   **실시간 핫 리로딩 (Hot-Reloading)**: AI가 도구를 생성하는 즉시 서버 측에서 구문 유효성을 검증하고 메모리에 로딩하여, **동일 대화 턴 내에서 새로 생성된 도구를 바로 호출(Call)**해 결과를 획득할 수 있습니다.
*   **자율 디버깅 빌드체인 (Self-Debugging Loop)**: 도구 스크립트 실행 과정에서 구문 오류나 런타임 예외가 발생할 경우, AI가 에러 트레이스를 분석하여 스스로 코드를 수정 및 재컴파일해 정상 동작할 때까지 실행을 반복합니다.
*   **안정적인 인자 전달**: 쉘 인용부호(Quotation) 및 이스케이프 버그를 차단하기 위해, 도구 인자를 임시 JSON 파일(`dynamic_tools/<name>_args.json`) 경로로 안전하게 전달하는 격리된 파이썬 실행 파이프라인을 지원합니다.
*   **Gemma-4 호환 토큰 클리너**: Gemma-4-it 모델의 특유 스트링 쿼트 토큰(`<|"` 및 `|>`)을 재귀적으로 파싱하고 제거하는 `clean_gemma_json` 파서가 탑재되어 있어, JSON 인자 추출 시 에러를 방지합니다.

### 3. ⚡ 시스템 프롬프트 우회 주입 (System Prompt Prepending)
*   LiteRT-LM C++ 엔진 내부의 `system_message_json` 처리 한계나 소형 모델의 시스템 프롬프트 무시 현상을 보완하기 위해, 첫 대화 시작 시 사용자의 첫 메시지 상단에 `soul.txt` + `tools.txt` 본문을 C++ 엔진 단에서 자동으로 결합해 주입합니다. 이를 통해 완벽한 AI 에이전트 지침 준수 성능을 보장합니다.

### 4. 🔌 Ollama / OpenAI API 호환 서버 모드
*   OpenAI 호환 API (`/v1/chat/completions`) 및 Ollama 호환 API (`/api/chat`, `/api/tags`) 엔드포인트를 완벽히 제공하여 로컬 개발 환경 및 외부 UI 클라이언트와 연동할 수 있습니다.

### 5. 🦀 Rust CLI 대화형 클라이언트 (`rust_ui`)
*   Tokio 비동기 런타임 기반의 가벼운 CLI 클라이언트 제공.
*   `/img <경로>` 명령어를 통한 이미지 파일 경로 자동 확장 및 멀티모달 프롬프트 전송.
*   `/settings` 대화형 설정을 통한 Temperature, Top-P, Top-K, Max Tokens, System Prompt의 동적 조회 및 저장.
*   수학식 드로잉 도구(`plot_function`) 호출 시 터미널 화면상에 점자(Braille) 그래픽 패턴(`BitCanvas`)을 실시간 렌더링.
*   **라인 스트리밍 파싱 보완**: TCP 스트림 레벨에서 끊겨서 전송되는 조각난 JSON 청크 단위를 안전하게 줄 단위 버퍼링하여 스트리밍 출력이 중간에 끊기거나 터지지 않도록 예외 처리 완료.

---

## 🛠️ 요구사항

*   **컴파일 도구**: GCC 13+ (C++20 지원), Bazelisk
*   **Rust 개발 환경**: Rust Toolchain (Cargo)
*   **공통**: Linux 환경 (Ubuntu 등)

---

## 📦 빌드하기 (Build)

### 1. LiteRT-LM C++ 엔진 공유 라이브러리 빌드 (최신 v0.13.1+ 기준)
Rust FFI가 시스템에 빌드된 최신 C++ 엔진 기능을 정상적으로 불러오기 위해 Bazel을 통해 공유 라이브러리(`.so`)를 직접 빌드합니다.
```bash
# Bazel로 C++ 공유 라이브러리 빌드
./bazelisk build :libLiteRtLmC.so
```

### 2. 빌드된 라이브러리를 카고 캐시 경로에 동기화
`litert-lm-sys` 크레이트 빌드 시 참조하는 로컬 캐시 경로에 방금 빌드한 최신 엔진을 덮어씌워 줍니다.
```bash
mkdir -p ~/.cache/litert-lm-sys/v0.10.2/x86_64-unknown-linux-gnu/
cp -f bazel-bin/libLiteRtLmC.so ~/.cache/litert-lm-sys/v0.10.2/x86_64-unknown-linux-gnu/libLiteRtLmC.so
```

### 3. Rust API 서버 빌드
```bash
cd rust_server
cargo build --release
```

### 4. Rust CLI 클라이언트 빌드
```bash
cd ../rust_ui
cargo build --release
```

---

## 💡 실행 및 사용법 (Usage)

### 🚀 Step 1. Rust API 서버 구동
반드시 최신 엔진 라이브러리가 로드될 수 있도록 `LD_LIBRARY_PATH` 환경 변수를 카고 캐시 경로로 지정하여 실행합니다.
```bash
cd rust_server
LD_LIBRARY_PATH=~/.cache/litert-lm-sys/v0.10.2/x86_64-unknown-linux-gnu cargo run --release -- --port 11435 /path/to/gemma-4-E2B-it.litertlm
```

### 🦀 Step 2. Rust CLI 클라이언트 구동
서버가 준비되면 다른 터미널에서 Rust CLI 클라이언트를 실행하여 서버와 대화를 시작합니다. (서버가 구동 중인 포트 `11435`를 매개변수로 지정합니다.)
```bash
cd rust_ui
cargo run --release -- --port 11435
```

*   **클라이언트 명령어**:
    *   `/clear`: 대화 기록 및 첨부된 이미지 초기화
    *   `/img <경로>`: 이미지 파일 첨부 (예: `/img ~/Desktop/test.jpg`)
    *   `/settings`: AI 모델 매개변수 설정 및 시스템 프롬프트 확인/변경
    *   `/exit`: 클라이언트 종료

---

## 📁 프로젝트 구조

*   `rust_server/`: Axum 기반 API 서버, FFI 링킹 인터페이스, 자율 에이전트 루프 모듈 (Rust)
*   `rust_ui/`: CLI 채팅 클라이언트 및 브라유 패턴 기반 그래픽 렌더러 구현 (Rust)
*   `main.cpp`: 레거시 C++ 서버 구현체
*   `tools.txt` & `soul.txt`: AI 에이전트의 규칙과 페르소나를 정의하는 프롬프트 파일
*   `tools.json`: 모델이 디폴트로 사용할 수 있는 외부 도구 스키마
*   `config.json`: LLM 기본 구동 파라미터 설정 파일
*   `dynamic_tools/`: AI가 실시간으로 코딩하여 저장 및 핫 로딩되는 파이썬 도구 보관소
*   `BUILD` & `WORKSPACE`: Bazel 빌드 시스템 설정 파일