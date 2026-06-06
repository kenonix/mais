# LiteRT-LM Multimodal API Server & Agentic Tool Synthesizer

Google의 **LiteRT-LM** (구 TensorFlow Lite-LM) C/C++ 엔진을 활용하여 로컬에서 멀티모달 LLM을 실행하고, AI가 스스로 도구를 개발 및 수정하여 문제를 해결하는 **자율적 동적 도구 빌드체인(Agentic Tool Synthesis)**이 내장된 고성능 C++ API 서버와 이를 연동하는 Rust CLI 클라이언트 프로젝트입니다.

---

## 🚀 주요 기능

### 1. 🛠️ AI 자율적 동적 도구 생성 및 활용 (Agentic Tool Synthesis)
*   **메타 도구 (`create_or_update_tool`)**: AI가 자신이 해결할 수 없는 복잡한 작업(로컬 파일 조회, 시스템 정보 검색, 외부 API 연동 등)을 마주했을 때, 스스로 적합한 파이썬 스크립트 도구를 작성하고 시스템에 실시간으로 등록합니다.
*   **실시간 핫 리로딩 (Hot-Reloading)**: AI가 도구를 생성하는 즉시 서버 측에서 구문 유효성을 검증하고 메모리에 로딩하여, **동일 대화 턴 내에서 새로 생성된 도구를 바로 호출(Call)**해 결과를 획득할 수 있습니다.
*   **자율 디버깅 빌드체인 (Self-Debugging Loop)**: 도구 스크립트 실행 과정에서 구문 오류나 런타임 예외가 발생할 경우, AI가 에러 트레이스를 분석하여 스스로 코드를 수정 및 재컴파일해 정상 동작할 때까지 실행을 반복합니다.
*   **안정적인 인자 전달**: 쉘 인용부호(Quotation) 및 이스케이프 버그를 차단하기 위해, 도구 인자를 임시 JSON 파일(`dynamic_tools/<name>_args.json`) 경로로 안전하게 전달하는 격리된 파이썬 실행 파이프라인을 지원합니다.
*   **Gemma-4 호환 토큰 클리너**: Gemma-4-it 모델의 특유 스트링 쿼트 토큰(`<|"` 및 `|>`)을 재귀적으로 파싱하고 제거하는 `clean_gemma_json` 파서가 탑재되어 있어, JSON 인자 추출 시 에러를 방지합니다.

### 2. ⚡ 시스템 프롬프트 우회 주입 (System Prompt Prepending)
*   LiteRT-LM C++ 엔진 내부의 `system_message_json` 처리 한계나 소형 모델의 시스템 프롬프트 무시 현상을 보완하기 위해, 첫 대화 시작 시 사용자의 첫 메시지 상단에 `system_prompt.txt` 본문을 C++ 엔진 단에서 자동으로 결합해 주입합니다. 이를 통해 완벽한 AI 에이전트 지침 준수 성능을 보장합니다.

### 3. 🔌 Ollama / OpenAI API 호환 서버 모드
*   OpenAI 호환 API (`/v1/chat/completions`) 및 Ollama 호환 API (`/api/chat`, `/api/tags`) 엔드포인트를 완벽히 제공하여 로컬 개발 환경 및 외부 UI 클라이언트와 연동할 수 있습니다.

### 4. 🦀 Rust CLI 대화형 클라이언트 (`rust_ui`)
*   Tokio 비동기 런타임 기반의 가벼운 CLI 클라이언트 제공.
*   `/img <경로>` 명령어를 통한 이미지 파일 경로 자동 확장 및 멀티모달 프롬프트 전송.
*   `/settings` 대화형 설정을 통한 Temperature, Top-P, Top-K, Max Tokens, System Prompt의 동적 조회 및 저장.
*   수학식 드로잉 도구(`plot_function`) 호출 시 터미널 화면상에 점자(Braille) 그래픽 패턴(`BitCanvas`)을 실시간 렌더링.

---

## 🛠️ 요구사항

*   **C++ 서버**: C++20 컴파일러 (GCC 13+), Bazelisk
*   **Rust 클라이언트**: Rust Toolchain (Cargo)
*   **공통**: 리눅스 환경

---

## 📦 빌드하기 (Build)

### 1. C++ 멀티모달 API 서버 빌드
```bash
./bazelisk build //:multimodal_server
```

### 2. Rust CLI 클라이언트 빌드
```bash
cd rust_ui
cargo build --release
```

---

## 💡 실행 및 사용법 (Usage)

### 🚀 Step 1. 자율 동적 도구 생성 에이전트 서버 구동
포트 `11435`에서 Ollama/OpenAI 호환 백엔드 서버를 작동시킵니다.
```bash
./bazel-bin/multimodal_server --port 11435 /path/to/gemma-4-E4B-it.litertlm
```

### 🦀 Step 2. Rust CLI 클라이언트 구동
서버가 준비되면 다른 터미널에서 Rust CLI 클라이언트를 실행하여 서버와 대화를 시작합니다.
```bash
cd rust_ui
./target/release/rust_ui --port 11435
```

*   **클라이언트 명령어**:
    *   `/clear`: 대화 기록 및 첨부된 이미지 초기화
    *   `/img <경로>`: 이미지 파일 첨부 (예: `/img ~/Desktop/test.jpg`)
    *   `/settings`: AI 모델 매개변수 설정 및 시스템 프롬프트 확인/변경
    *   `/exit`: 클라이언트 종료

---

## 📁 프로젝트 구조

*   `main.cpp`: 메인 서버 로직, 컴파일러 프로세스 파이프라인, HTTP API 서버 코어 구현체
*   `rust_ui/`: CLI 채팅 클라이언트 및 브라유 패턴 기반 그래픽 렌더러 구현 (Rust)
*   `tools.json`: 모델이 디폴트로 사용할 수 있는 외부 도구 스키마
*   `system_prompt.txt`: 자율 도구 빌드 및 잡담 금지 규칙 등이 적힌 에이전트용 마스터 프롬프트
*   `config.json`: LLM 기본 구동 파라미터 설정 파일
*   `dynamic_tools/`: AI가 실시간으로 코딩하여 저장 및 핫 로딩되는 파이썬 도구 보관소
*   `BUILD` & `WORKSPACE`: Bazel 빌드 시스템 설정 파일