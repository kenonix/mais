// 표준 라이브러리 파일 시스템 라이브러리를 사용합니다.
use std::fs;
// 외부 셸 명령 및 서브 프로세스 실행을 위해 Command 라이브러리를 임포트합니다.
use std::process::Command;
// JSON 가공을 위해 serde_json 크레이트를 가져옵니다.
use serde_json;
// 프로젝트 내부 유틸리티 모듈(utils)에서 필요한 헬퍼 함수들을 가져옵니다.
use crate::utils::{get_workspace_root, log_tool_call, clean_gemma_json};

// 동적으로 생성한 파이썬 스크립트 파일을 실행하고 실행 표준출력을 결과 문자열로 리턴해 주는 함수입니다.
pub fn execute_dynamic_tool(name: &str, arguments_json: &str, raw_args: &str) -> String {
    // 워크스페이스의 루트 경로를 식별합니다.
    let root = get_workspace_root();
    // 동적 파이썬 도구들이 밀집해 저장되는 폴더의 전체 물리 경로를 생성합니다.
    let dynamic_tools_dir = root.join("dynamic_tools");
    // 동적 도구 저장소 디렉토리가 생성되지 않았다면 하위 폴더까지 한 번에 자동 생성합니다.
    let _ = fs::create_dir_all(&dynamic_tools_dir);
    // 동적 스크립트에 아규먼트로 전달할 파라미터 임시 JSON 파일 경로를 정의합니다.
    let args_path = dynamic_tools_dir.join(format!("{}_args.json", name));
    // 파라미터 데이터를 임시 JSON 파일에 저장하고, 저장 중 IO 실패 시 에러 내용을 즉시 로깅하고 보고합니다.
    if let Err(e) = fs::write(&args_path, arguments_json) {
        // 에러 로그 메시지를 구성합니다.
        let err_msg = format!("Error: Failed to create arguments file for tool execution: {}", e);
        // 오류 상세 내역을 tool_calls.log 파일에 영구 기록합니다.
        log_tool_call(name, raw_args, arguments_json, -1, &err_msg);
        // 에러 통지값을 결과 문자열로 즉시 리턴합니다.
        return err_msg;
    }
    
    // 동작시킬 파이썬 대상 파일의 실제 경로를 획득합니다.
    let py_path = dynamic_tools_dir.join(format!("{}.py", name));
    // 셸 터미널 환경에서 실행할 파이썬 명령어와 인자 파일 경로를 한 문장으로 연결 포맷팅합니다.
    let cmd = format!("python3 {} {} 2>&1", py_path.to_string_lossy(), args_path.to_string_lossy());
    // 시스템 리눅스 기본 셸인 sh 프로그램 객체를 생성합니다.
    let mut command = Command::new("sh");
    // 실행 아규먼트로 셸 스크립트 모드인 -c와 조립 완료한 명령 라인 cmd 문자열을 전달합니다.
    command.arg("-c").arg(&cmd);
    
    // 서브 프로세스를 기동하고 실행이 끝날 때까지 대기하여 그 결과를 가로챕니다.
    let output = match command.output() {
        // 프로세스 실행 완료에 도달했을 때
        Ok(out) => {
            // 프로세스가 남기고 간 최종 종료 코드(exit status)를 가져옵니다. 비정상 종료 시 -1을 부여합니다.
            let exit_code = out.status.code().unwrap_or(-1);
            // 프로세스 표준출력/표준에러 결과를 UTF-8 손실 복구 모드를 통해 Rust 문자열로 변환합니다.
            let stdout_str = String::from_utf8_lossy(&out.stdout).into_owned();
            // 도구 실행 내역과 리턴 상태를 기록하기 위해 로그 기록 유틸리티를 호출합니다.
            log_tool_call(name, raw_args, arguments_json, exit_code, &stdout_str);
            // 최종 가공 완료한 결과물을 받아둡니다.
            stdout_str
        }
        // 아예 명령어 구동 자체가 시스템 상에서 차단되었거나 에러가 났을 때
        Err(e) => {
            // 에러 출력용 디버깅 텍스트를 준비합니다.
            let err_msg = format!("Error: Failed to execute tool command: {}", e);
            // 오류가 발생한 상태로 실행 실패 내역을 로그에 납부합니다.
            log_tool_call(name, raw_args, arguments_json, -1, &err_msg);
            // 에러 정보를 넘겨줍니다.
            err_msg
        }
    };
    
    // 파이썬 실행 종료 후 자원 정리를 위해 생성했던 임시 인자 JSON 파일을 로컬 스토리지에서 삭제합니다.
    let _ = fs::remove_file(&args_path);
    // 도구의 출력값을 최종 리턴합니다.
    output
}

// 모델이 보낸 도구 명칭(name)과 매개변수 본문을 검사하여 알맞은 도구 로직으로 연결해주는 라우팅 허브 함수입니다.
pub fn execute_tool(name: &str, arguments_json: &str) -> String {
    // 서버 개발용 표준 콘솔에 현재 호출된 도구 정보와 원본 수신 인자들을 프린트합니다.
    println!("[시스템] ExecuteTool 호출: name={}, args={}", name, arguments_json);
    // 전달된 인자 문자열을 serde_json을 통해 동적 JSON 객체(serde_json::Value)로 가공하며, 분석 불능 시 에러를 반환합니다.
    let mut args_j: serde_json::Value = match serde_json::from_str(arguments_json) {
        // 파싱이 통과된 경우
        Ok(v) => v,
        // 문법적 오류로 해독에 실패한 경우 에러 기록 후 종결 처리합니다.
        Err(e) => {
            // 에러 텍스트 생성
            let err_msg = format!("Error parsing arguments JSON: {}", e);
            // 로그북에 실패 사유를 추가합니다.
            log_tool_call(name, arguments_json, "{}", -1, &err_msg);
            // 분석 불가 응답 리턴
            return err_msg;
        }
    };
    // 모델의 원시 출력 구조에 섞인 장식 태그를 걸러서 순수한 JSON 형태로 복제 클리닝해 줍니다.
    clean_gemma_json(&mut args_j);
    // 정제가 완료된 깔끔한 인자 JSON 객체를 한 줄 텍스트 문자열 포맷으로 변환해 둡니다.
    let cleaned_args_str = args_j.to_string();
    
    // 프로젝트 루트 폴더 주소를 구합니다.
    let root = get_workspace_root();
    
    // 모델의 지시가 만약 신규 도구 동적 생성이거나 갱신(create_or_update_tool) 건일 때의 전용 분기 처리입니다.
    if name == "create_or_update_tool" {
        // 인자 중 "name"을 추출하며 유실 시 공백 문자열로 세팅합니다.
        let tool_name = args_j.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        // 인자 중 도구의 용도를 설명하는 "description" 필드를 수거합니다.
        let tool_desc = args_j.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        // 모델이 규정한 도구의 파라미터 구조 명세 객체를 가져오며 없으면 빈 오브젝트 맵으로 세팅합니다.
        let tool_params = args_j.get("parameters").cloned().unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        // 생성할 파이썬 소스 코드 텍스트 덩어리를 받아냅니다.
        let raw_code = args_j.get("code").and_then(|v| v.as_str()).unwrap_or("").to_string();
        
        // 1단계: 모델이 습관적으로 코드 시작 부분에 마크다운 블록 펜스(```python)를 둘렀다면 해당 태그를 가위질해 줍니다.
        let stripped_code = if raw_code.trim().starts_with("```") {
            // 줄바꿈 단위로 코드 라인들을 분해하여 모읍니다.
            let lines: Vec<&str> = raw_code.lines().collect();
            // 맨 윗줄이 펜스 선언인지 점검하여 시작 라인 인덱스(0 또는 1)를 책정합니다.
            let start = if lines.first().map_or(false, |l| l.trim().starts_with("```")) { 1 } else { 0 };
            // 맨 끝줄이 블록 마감 펜스(```)인지 점검하여 닫는 라인 인덱스를 잡습니다.
            let end = if lines.last().map_or(false, |l| l.trim() == "```") { lines.len() - 1 } else { lines.len() };
            // 펜스들을 걷어내고 내용물 소스 코드만 다시 한 덩어리로 직조합니다.
            lines[start..end].join("\n")
        } else {
            // 마크다운 형태가 아닐 경우 원본 코드를 그대로 넘겨 줍니다.
            raw_code.clone()
        };
        
        // 2단계: JSON 통신 과정 중 이스케이프 문자열 상태로 압축되어 들어온 \n, \t, \r 등의 리터럴 기호를 정규 줄바꿈/탭 문자로 치환 가공합니다.
        let tool_code = stripped_code
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\r", "\r")
            .replace("\\\\", "\\");
        
        // 3단계: 소스 코드가 아직 일렬 종대로 연결된 외줄이며 세미콜론(;)이 대량 검출되는 경우, 가독성 확보를 위해 개행문자(\n)로 해체합니다.
        let tool_code = if !tool_code.contains('\n') && tool_code.matches(';').count() >= 2 {
            // 세미콜론을 모두 줄바꿈으로 변경
            tool_code.replace("; ", "\n").replace(";", "\n")
        } else {
            // 일반적인 형태인 경우 통과
            tool_code
        };
        
        // 터미널 디버깅 화면에 지금 등록하려는 도구의 이름과 함께 적재 시작을 출력합니다.
        println!("[시스템] 서버 측 Tool Call 실행: create_or_update_tool(name: \"{}\")", tool_name);
        // 소스 코드의 전두엽부 300글자만 검사해 잘 작성되었는지 콘솔에 미리보기로 노출시킵니다.
        println!("[시스템] 도구 코드 미리보기 (처음 300자):\n{}", tool_code.chars().take(300).collect::<String>());
        
        // 도구 이름 정보나 실제 실행할 파이썬 로직 코드가 비어 있다면 원천 생성 불가로 판단하고 오류 문서를 리턴합니다.
        if tool_name.is_empty() || tool_code.is_empty() {
            // 클라이언트용 오류 JSON 문서를 만듭니다.
            let err_msg = "{\"status\": \"error\", \"message\": \"도구 이름(name)과 코드(code)는 필수 항목입니다.\"}".to_string();
            // 오류 사실을 영구 보존 로그에 기록합니다.
            log_tool_call(name, arguments_json, &cleaned_args_str, -1, &err_msg);
            // 오류를 전송
            return err_msg;
        }
        
        // 동적 도구들이 입주할 디렉토리 경로를 마련합니다.
        let dynamic_tools_dir = root.join("dynamic_tools");
        // 해당 물리 디렉토리가 부재 중일 시 생성합니다.
        let _ = fs::create_dir_all(&dynamic_tools_dir);
        // 지정된 도구명으로 생성할 파이썬 소스 스크립트 파일명 경로를 생성합니다.
        let py_path = dynamic_tools_dir.join(format!("{}.py", tool_name));
        // 스크립트 물리 파일을 새로 만들고 코드를 씁니다. 만일 물리적 디스크 오버플로우 등의 이유로 쓰기 실패 시 예외 처리합니다.
        if let Err(e) = fs::write(&py_path, &tool_code) {
            // 실패 원인을 파악한 뒤 보고용 JSON 구조 문장을 마련합니다.
            let err_msg = format!("{{\"status\": \"error\", \"message\": \"스크립트 파일 저장 실패: {}\"}}", e);
            // 도구 등록 에러를 아카이빙합니다.
            log_tool_call(name, arguments_json, &cleaned_args_str, -1, &err_msg);
            // 상태 보고
            return err_msg;
        }
        
        // 등록된 스크립트 코드 내부가 문법적으로 완전무결한지 파이썬 사전 컴파일 체크를 구동해봅니다.
        let check_cmd = format!("python3 -m py_compile {} 2>&1", py_path.to_string_lossy());
        // 내부 체크용 셸 커맨드 준비
        let mut check_process = Command::new("sh");
        // 파라미터를 입력
        check_process.arg("-c").arg(&check_cmd);
        
        // 검사 셸을 구동하여 syntax error가 보고되는지 실시간 확인을 진행합니다.
        match check_process.output() {
            // 정상적으로 셸 프로세스가 끝났을 시
            Ok(out) => {
                // 검사 컴파일의 리턴 코드가 0이 아닌 에러 값이거나, 출력창에 에러 스택이 출력되었는지 스캔합니다.
                let exit_code = out.status.code().unwrap_or(-1);
                // 에러 보고 문자열의 공백을 자릅니다.
                let check_res = String::from_utf8_lossy(&out.stdout).trim().to_string();
                // 코드가 깨졌거나 출력창이 조용하지 않다면 문법상 하자가 있다고 선고합니다.
                if exit_code != 0 || !check_res.is_empty() {
                    // 문법 고장 알림 JSON
                    let err_msg = format!("{{\"status\": \"error\", \"message\": \"파이썬 문법 검사 실패: {}\"}}", check_res);
                    // 로그에 문법 에러 스택을 통째로 올립니다.
                    log_tool_call(name, arguments_json, &cleaned_args_str, exit_code, &err_msg);
                    // 고장 내역 전송
                    return err_msg;
                }
            }
            // 셸 커맨드 시작 자체가 불가한 경우
            Err(e) => {
                // 하드웨어 통제 장애 등 상태 경고 JSON 작성
                let err_msg = format!("{{\"status\": \"error\", \"message\": \"컴파일러 구동 실패: {}\"}}", e);
                // 기록 작성
                log_tool_call(name, arguments_json, &cleaned_args_str, -1, &err_msg);
                // 보고 리턴
                return err_msg;
            }
        }
        
        // 문법 통과 완료 후, 레지스트리 레코드 관리 파일(registry.json)을 찾아 도구 명세를 올려줍니다.
        let registry_path = dynamic_tools_dir.join("registry.json");
        // 파일에 담긴 기존 레지스트리를 읽어 JSON 구조로 변환하며 누락 시 빈 맵을 준비해 둡니다.
        let mut registry: serde_json::Value = if let Ok(content) = fs::read_to_string(&registry_path) {
            // 기존 레코드 읽기
            serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
        } else {
            // 신규 레코드용 맵 생성
            serde_json::Value::Object(serde_json::Map::new())
        };
        
        // 최종 주입해 줄 매개변수 양식을 세팅합니다.
        let mut final_params = tool_params;
        // 모델이 스키마를 잘못 선언했거나 누락 시, 기본 오브젝트 명세 포맷을 보강 부여하여 안착시킵니다.
        if final_params.is_null() || final_params.as_object().map_or(true, |m| m.is_empty()) {
            // 기본 스키마 JSON 빌드
            final_params = serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": true
            });
        }

        // 수집한 레지스트리 데이터가 정상 맵 구조이면 새로운 동적 도구의 인터페이스 정보를 삽입 혹은 덮어씁니다.
        if let Some(obj) = registry.as_object_mut() {
            // 도구명 키 아래에 올바른 JSON API 명세를 구축해 넣습니다.
            obj.insert(tool_name.clone(), serde_json::json!({
                // 타입을 함수로 명시
                "type": "function",
                // 상세 속성들을 기록
                "function": {
                    "name": tool_name,
                    "description": tool_desc,
                    "parameters": final_params
                }
            }));
        }
        
        // 변경 완료된 도구 인터페이스 리스트를 정돈된(pretty) 텍스트 데이터로 포맷팅하여 디스크 파일에 영구 기록합니다.
        if let Ok(reg_str) = serde_json::to_string_pretty(&registry) {
            // 파일 업데이트
            let _ = fs::write(&registry_path, reg_str);
        }
        
        // 모델에게 도구 제작이 무사 완료되었음을 선포하는 획기적이고 구체적인 성공 통보 문장을 완성합니다.
        let success_msg = format!("{{\"status\": \"success\", \"message\": \"도구 '{}' 등록 완료. 텍스트를 출력하지 말고 즉시 이 도구를 호출하여 사용자의 원래 요청을 수행하십시오.\"}}", tool_name);
        // 대장 로그에 등록 보고를 완수합니다.
        log_tool_call(name, arguments_json, &cleaned_args_str, 0, &success_msg);
        // 선포문 리턴
        return success_msg;
    }
    
    // 등록 요청이 아닌 일반적인 커스텀 동적 도구 호출 명령일 때 처리
    let py_path = root.join("dynamic_tools").join(format!("{}.py", name));
    // 해당 명칭과 매칭되는 파이썬 파일이 dynamic_tools 디렉토리에 안착하고 있는지 확인합니다.
    if py_path.exists() {
        // 시스템 및 디버거 창에 해당 도구가 격발되었음을 출력합니다.
        println!("[시스템] 서버 측 동적 Tool Call 실행: {}", name);
        // 준비된 동적 도구 전용 파이썬 구동기를 호출해 그 출력 텍스트를 그대로 최종 회신합니다.
        return execute_dynamic_tool(name, &cleaned_args_str, arguments_json);
    }
    
    // 어떤 매칭 도구도 탐색되지 않았을 때 반환하는 에러 응답 문구입니다.
    "Unknown tool".to_string()
}
