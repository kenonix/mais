// 표준 라이브러리의 파일 시스템 제어 라이브러리를 임포트합니다.
use std::fs;
// 표준 라이브러리의 경로 처리를 위한 PathBuf 타입을 가져옵니다.
use std::path::PathBuf;

// AI 성격 정의가 들어있는 기본 소울 텍스트 파일의 이름 정의입니다.
pub const SOUL_FILE: &str = "soul.txt";
// 외부 등록 도구들의 설명 가이드가 위치한 텍스트 파일의 이름 정의입니다.
pub const TOOLS_FILE: &str = "tools.txt";

// 소울 파일이 소실되거나 비어있을 때 사용되는 하드코딩된 기본 한국어 소울 프롬프트입니다.
pub const DEFAULT_SOUL: &str = "당신의 이름은 AI입니다.\n한국어만 사용하며, 친절하고 명확하게 답변합니다.\n사용자에게 보이는 답변은 자연스러운 평문을 우선합니다.\n수학적 그래프 시각화가 필요할 경우 수식을 평문으로 설명하고, 필요한 경우 그래프 도구를 사용하세요.";

// 현재 시각 데이터를 ISO 8601 규격 문자열(마이크로초 단위 정밀도 및 UTC 표시)로 리턴하는 유틸리티 함수입니다.
pub fn get_iso8601_now() -> String {
    // Utc::now()로 현재 타임스탬프를 잡고 RFC 3339 규격 옵션에 맞추어 문자열을 포맷팅합니다.
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

// 프로젝트의 루트 디렉토리를 찾아 반환해 주는 함수입니다.
pub fn get_workspace_root() -> PathBuf {
    // env::current_dir()을 통해 현재 이 서버 실행 명령이 들어간 디렉토리를 찾아오고 에러가 나면 현재 경로(.)로 대체합니다.
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // 만약 현재 작업 디렉토리에 soul.txt나 tools.json이 자리하고 있다면 이곳이 최상위 루트 디렉토리입니다.
    if current_dir.join(SOUL_FILE).exists() || current_dir.join("tools.json").exists() {
        // 현재 경로를 곧바로 루트 경로로써 반환합니다.
        return current_dir;
    }
    // 그렇지 않은 경우 상위 디렉토리(..)에 이 파일들이 있는지 살펴봅니다.
    let parent_dir = current_dir.join("..");
    // 상위 디렉토리에 필수 설정 파일들이 존재하면 거기가 빌드 루트입니다.
    if parent_dir.join(SOUL_FILE).exists() || parent_dir.join("tools.json").exists() {
        // 상위 디렉토리의 절대 경로 객체를 반환합니다.
        return parent_dir;
    }
    // 어느 쪽도 발견되지 않을 시 fallback하여 현재 디렉토리 경로를 넘겨줍니다.
    current_dir
}

// soul.txt와 tools.txt 파일의 텍스트 콘텐츠를 읽고 조립하여 최종 시스템 프롬프트를 만드는 함수입니다.
pub fn load_system_prompt() -> String {
    // 루트 디렉토리 경로를 찾아 변수에 저장합니다.
    let root = get_workspace_root();
    // 소울 파일이 존재해야 하는 절대 경로를 계산합니다.
    let soul_path = root.join(SOUL_FILE);
    // 소울 텍스트를 열어서 읽으며, 실패(에러) 시 기본 하드코딩 문구 문자열로 대체하여 확보합니다.
    let soul = fs::read_to_string(&soul_path).unwrap_or_else(|_| DEFAULT_SOUL.to_string());
    // 만약 읽어들인 텍스트가 줄바꿈이나 공백 문자로만 가득 차 있다면 기본 안내 문구로 교체합니다.
    let soul = if soul.trim().is_empty() { DEFAULT_SOUL.to_string() } else { soul };
    
    // 도구 지침 텍스트가 저장되어 있는 경로를 계산합니다.
    let tools_path = root.join(TOOLS_FILE);
    // 도구 지침 파일을 열어 읽어들이고 오류 발생 시 기본 빈 문자열을 사용합니다.
    let tools = fs::read_to_string(&tools_path).unwrap_or_default();
    // 도구 설명 텍스트가 비어 있는 경우 성격 프롬프트만 그대로 리턴합니다.
    if tools.trim().is_empty() {
        // 성격 설정본을 돌려줍니다.
        soul
    } else {
        // 성격 문안 아래에 두 줄을 띄워 도구 사용 행동 지침을 연결한 결과값을 포맷하여 리턴합니다.
        format!("{}\n\n{}", soul, tools)
    }
}

// 정적으로 빌드된 tools.json 목록과 동적으로 생성된 dynamic_tools/registry.json 목록을 합산하는 함수입니다.
pub fn load_merged_tools() -> String {
    // 현재 워크스페이스의 루트 경로를 알아옵니다.
    let root = get_workspace_root();
    // 정적 도구 정의 목록이 보관된 tools.json의 물리 경로를 세팅합니다.
    let tools_json_path = root.join("tools.json");
    // tools.json 파일을 읽어서 유효한 JSON 배열로 파싱을 시도하고 실패하면 빈 배열 json([]) 객체로 정의합니다.
    let static_tools: serde_json::Value = if let Ok(content) = fs::read_to_string(&tools_json_path) {
        // serde_json의 문자열 파서 기능을 호출합니다.
        serde_json::from_str(&content).unwrap_or(serde_json::json!([]))
    } else {
        // 파일 읽기 실패 시 빈 구조를 생성합니다.
        serde_json::json!([])
    };
    // 파싱된 정적 도구 리스트를 Rust의 벡터(vector) 데이터 구조로 풀어내 복제하여 보관합니다.
    let mut static_tools_arr = if let Some(arr) = static_tools.as_array() {
        // 이미 JSON 배열인 경우에 clone으로 소유권을 가집니다.
        arr.clone()
    } else {
        // 배열 구조가 아닐 경우 안전하게 빈 벡터를 준비합니다.
        vec![]
    };
    
    // 동적으로 추가되어 저장된 dynamic_tools/registry.json 레지스트리 파일 경로를 세팅합니다.
    let registry_path = root.join("dynamic_tools/registry.json");
    // 레지스트리를 텍스트로 읽어서 오브젝트(map형식)로 파싱하고 안되면 빈 객체를 생성해 대입합니다.
    let dynamic_registry: serde_json::Value = if let Ok(content) = fs::read_to_string(&registry_path) {
        // JSON 문자열 변환기를 실행합니다.
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        // 파일 탐색에 실패하거나 유실되었을 때 기본 맵을 형성합니다.
        serde_json::json!({})
    };
    
    // 파싱된 동적 도구들이 맵 오브젝트 형태로 들어있는지 조사합니다.
    if let Some(obj) = dynamic_registry.as_object() {
        // 맵 내부를 순회하며 개별 등록된 도구 사양들(spec)을 꺼냅니다.
        for (_, spec) in obj {
            // 복사된 정적 도구 벡터 끝에 이 동적 도구 세부 규격을 추가합니다.
            static_tools_arr.push(spec.clone());
        }
    }
    
    // 모든 도구들을 하나로 합친 JSON 배열 값을 최종적으로 원시 텍스트 문자열(string)로 직렬화하여 반환합니다.
    serde_json::json!(static_tools_arr).to_string()
}

// Gemma 언어 모델이 임의로 생성하는 내부 특수 JSON 구문 장식 태그(<|'|>)를 정제해주는 유틸 함수입니다.
pub fn clean_gemma_json(v: &mut serde_json::Value) {
    // 들어온 JSON의 타입 형태에 부합하도록 패턴 매칭을 실시합니다.
    match v {
        // 데이터가 문자열 형태인 경우
        serde_json::Value::String(s) => {
            // 만약 이상한 인용 토큰 접두사(<|'|>)로 시작하고 있다면 정제합니다.
            if s.starts_with("<|\"|>") {
                // 해당 특수 토큰 5글자 뒤쪽의 문자열만 잘라내 덮어씁니다.
                *s = s[5..].to_string();
            }
            // 마찬가지로 특수 접미사(<|'|>)로 끝맺어지고 있는 경우 끝부분도 잘라냅니다.
            if s.ends_with("<|\"|>") && s.len() >= 5 {
                // 뒤쪽의 5글자를 빼고 원본 영역만 남깁니다.
                *s = s[..s.len() - 5].to_string();
            }
        }
        // 데이터가 key-value 오브젝트 맵 형태인 경우 내부 원소들을 순회 정제합니다.
        serde_json::Value::Object(map) => {
            // 오브젝트 내의 가변 참조를 순회하며 재귀적으로 정제 함수를 돌립니다.
            for (_, val) in map.iter_mut() {
                // 재귀 호출
                clean_gemma_json(val);
            }
        }
        // 데이터가 배열 리스트 구조인 경우 원소마다 순회하며 태그 정제를 돌립니다.
        serde_json::Value::Array(arr) => {
            // 가변 순회
            for val in arr.iter_mut() {
                // 재귀 호출
                clean_gemma_json(val);
            }
        }
        // 숫자, 논리값 등 문자열과 무관한 원시 유형들은 별다른 변경 없이 건너뜁니다.
        _ => {}
    }
}

// Gemma 등 언어 모델 출력 청크(JSON 문자열) 내부에서 실제로 출력된 텍스트 필드를 추출하는 함수입니다.
pub fn extract_text_from_chunk(chunk: &str) -> String {
    // 청크 문자열을 구문 분석하여 JSON 데이터 형태로 안전하게 로드해 봅니다.
    if let Ok(j) = serde_json::from_str::<serde_json::Value>(chunk) {
        // 내부 키 중에서 "content" 필드가 자리잡고 있는지 조회합니다.
        if let Some(content) = j.get("content") {
            // 해당 콘텐츠 필드가 문자열 값인 경우 바로 해당 텍스트를 돌려줍니다.
            if let Some(s) = content.as_str() {
                // String 타입 소유권 이전
                return s.to_string();
            }
            // 만약 콘텐츠 필드가 배열 형식으로 인코딩되어 있다면 루프를 돌면서 텍스트들을 융합해 줍니다.
            if let Some(arr) = content.as_array() {
                // 합산을 위해 빈 String 버퍼 생성
                let mut res = String::new();
                // 원소별 스캔
                for item in arr {
                    // 각 원소 내부에 "text" 키를 추출하여 텍스트 데이터가 있다면 가산합니다.
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        // 결과 버퍼에 추가
                        res.push_str(text);
                    }
                }
                // 축적 완료된 문자열 최종 반환
                return res;
            }
        }
    }
    // 데이터 추출에 실패하거나 대상 필드가 비어 있으면 빈 텍스트를 돌려줍니다.
    String::new()
}

// 텍스트 블록 안에서 괄호의 중첩 깊이를 추적하여 올바른 형식의 중괄호 JSON 데이터 영역을 모두 발굴해내는 분석기 함수입니다.
pub fn find_all_json_blocks(text: &str) -> Vec<(usize, usize, serde_json::Value)> {
    // 추출에 성공한 JSON의 시작 바이트 위치, 끝 바이트 위치 및 변환 값을 저장할 결과 벡터를 초기화합니다.
    let mut blocks = Vec::new();
    // UTF-8 바이트 인덱싱 문제를 방지하고자 입력 텍스트 전체를 유니코드 글자(char) 배열 벡터로 만듭니다.
    let chars: Vec<char> = text.chars().collect();
    // 글자 배열의 총 개수를 구해 둡니다.
    let len = chars.len();
    
    // 시작 커서 위치를 나타내는 인덱스 변수를 0으로 설정합니다.
    let mut i = 0;
    // 배열의 끝에 도달하기 전까지 순회를 계속합니다.
    while i < len {
        // 여는 중괄호('{')를 처음 만나게 되면 JSON 블록 후보로 삼아 스캔을 개시합니다.
        if chars[i] == '{' {
            // 중괄호 중첩 레벨을 나타내는 깊이를 1로 초기화합니다.
            let mut depth = 1;
            // 현재 스캔 커서가 쌍따옴표 내 문자열 영역을 훑는 중인지를 나타내는 플래그입니다.
            let mut in_string = false;
            // 문자열 내부 역슬래시 이스케이프 패턴('\') 감지 여부를 기록하는 플래그입니다.
            let mut escape = false;
            // 스캔 진행을 기록하기 위해 'i + 1' 위치부터 시작하는 탐색 포인터를 지정합니다.
            let mut j = i + 1;
            // 탐색 포인트가 영역 밖으로 벗어나지 않았고 중괄호가 짝을 맞춰 완전히 닫히기 전(depth > 0)까지 진행합니다.
            while j < len && depth > 0 {
                // 탐색 포인트에 있는 현재 유니코드 문자열 글자를 받아옵니다.
                let c = chars[j];
                // 직전 루프에서 이스케이프 선언문이 들어온 경우 처리
                if escape {
                    // 이스케이프 기호를 사용했으므로 플래그를 원복하고 다음 한 글자는 특수 기호 대신 단순 문자로 취급합니다.
                    escape = false;
                } else if c == '\\' {
                    // 역슬래시를 만난 경우 바로 다음의 기호를 탈출(escape) 처리하도록 준비합니다.
                    escape = true;
                } else if c == '"' {
                    // 탈출 문자가 아닌 일반 쌍따옴표를 발견했을 때에는 문자열 캡슐화 영역 상태(in_string)를 반전시킵니다.
                    in_string = !in_string;
                } else if !in_string {
                    // 문자열 영역 밖에서 돌아다니고 있을 때에만 중괄호의 열림 닫힘에 따른 깊이 수준을 가감합니다.
                    if c == '{' {
                        // 중첩이 추가됨을 기록
                        depth += 1;
                    } else if c == '}' {
                        // 중첩 괄호 하나가 종결되었음을 기록
                        depth -= 1;
                    }
                }
                // 다음 칸으로 탐색 검사 커서를 이동합니다.
                j += 1;
            }
            
            // 모든 괄호의 짝이 맞아서 깊이(depth) 수치가 0에 도달하여 무사히 종결 스캔되었는지 점검합니다.
            if depth == 0 {
                // 시작 글자 인덱스를 문자열 상 바이트 단위 물리 오프셋 위치값으로 변환합니다.
                let start_byte = text.char_indices().nth(i).map(|(idx, _)| idx).unwrap_or(0);
                // 스캔 종료 위치도 마찬가지로 문자열 내부의 실제 바이트 오프셋 주소로 치환합니다.
                let end_byte = text.char_indices().nth(j).map(|(idx, _)| idx).unwrap_or(text.len());
                // 해당 범위가 안전하게 텍스트 내 서브 스트링 범위로 슬라이싱되는지 살핍니다.
                if let Some(candidate) = text.get(start_byte..end_byte) {
                    // Gemma 언어 모델이 파라미터 따옴표를 <|\\\"|> 또는 <|\"|> 등으로 잘못 이스케이프한 패턴들을 고쳐줍니다.
                    let cleaned = candidate
                        .replace("<|\\\"|>", "\\\"")
                        .replace("<|\"|>", "\\\"")
                        .replace("\\\\\"", "\\\"");
                    
                    // 정제가 완료된 텍스트가 완전한 구조의 JSON 객체 유형으로 정상 해석되는지 점검해 봅니다.
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&cleaned) {
                        // 변환에 안착한 경우 시작 위치, 끝 위치, 파싱한 JSON 결과 값을 blocks 결과 목록에 삽입합니다.
                        blocks.push((start_byte, end_byte, parsed));
                        // 스캔에 성공했으므로 메인 커서 위치 i를 이번 스캔 종료점 j로 건너뛰어 중복 검출을 차단합니다.
                        i = j;
                        // 다음 루프로 스킵 진행
                        continue;
                    }
                }
            }
        }
        // 블록 감지 실패 시 다음 문자 한 칸을 점진적으로 이동하여 체크 프로세스를 반복합니다.
        i += 1;
    }
    // 탐색된 모든 JSON 매핑 영역 구조 벡터를 반환합니다.
    blocks
}

// 모델이 자유롭게 출력한 JSON 데이터 포맷을 정규 도구 규격(create_or_update_tool 또는 커스텀 호출)으로 표준화하는 함수입니다.
pub fn normalize_tool_call(val: &serde_json::Value) -> Option<(String, serde_json::Value)> {
    // 들어온 데이터가 JSON 맵 오브젝트인지 확인하고 아니면 반환값 없이 종결합니다.
    let obj = val.as_object()?;
    
    // 도구 코드를 가지고 있는지 알려주는 체크 변수입니다. key값으로 code나 tool_code가 있는지 검사합니다.
    let has_code = obj.contains_key("code") || obj.contains_key("tool_code");
    // 도구 이름 정보를 담고 있는지 여부입니다. key값으로 name이나 tool_name이 있는지 검사합니다.
    let has_name = obj.contains_key("name") || obj.contains_key("tool_name");
    
    // 이름과 도구 소스 코드를 모두 포함하고 있으면 "도구 생성/업데이트(create_or_update_tool)" 요청으로 강제 정규화해줍니다.
    if has_name && has_code {
        // 정규 구조에 담길 새로운 데이터 맵을 만듭니다.
        let mut normalized_args = serde_json::Map::new();
        
        // 이름 정보를 추출해내거나 없을 때에는 기본 null을 준비합니다.
        let name_val = obj.get("name").or(obj.get("tool_name")).cloned().unwrap_or(serde_json::Value::Null);
        // 도구 설명글을 관련 가능성 있는 여러 키(description, tool_description 등)로부터 수집합니다.
        let desc_val = obj.get("description")
            .or(obj.get("tool_description"))
            .or(obj.get("desc"))
            .or(obj.get("tool_desc"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        // 소스 코드 값을 확보합니다.
        let code_val = obj.get("code").or(obj.get("tool_code")).or(obj.get("script")).cloned().unwrap_or(serde_json::Value::Null);
        
        // 도구 실행에 필요한 매개변수 양식(parameters)을 가공합니다. 누락 시 기본 오브젝트 양식 구조를 만듭니다.
        let params_val = obj.get("parameters").or(obj.get("tool_parameters")).cloned().unwrap_or_else(|| {
            // 빈 JSON 객체 선언
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": true
            })
        });
        
        // 정형화한 맵 오브젝트 내에 해당 정보 값들을 가지런히 추가합니다.
        normalized_args.insert("name".to_string(), name_val);
        // 가공된 상세 정보를 삽입합니다.
        normalized_args.insert("description".to_string(), desc_val);
        // 소스 코드도 기입합니다.
        normalized_args.insert("code".to_string(), code_val);
        // 파라미터 구조체도 매핑합니다.
        normalized_args.insert("parameters".to_string(), params_val);
        
        // 최종적으로 create_or_update_tool 이름과 함께 정규화 가공된 매개변수 패킷 데이터를 리턴합니다.
        return Some(("create_or_update_tool".to_string(), serde_json::Value::Object(normalized_args)));
    }
    
    // 도구 이름 키워드만 정의되어 있는 일반 커스텀 실행 성격의 호출 문장인 경우
    if has_name {
        // 도구의 실명을 추출하여 확실한 문자열 타입으로 저장합니다.
        let name_str = obj.get("name").or(obj.get("tool_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
            
        // 이름이 빈칸이 아님을 확실히 하고 다음 처리를 합니다.
        if !name_str.is_empty() {
            // 인자 목록을 뜻할 만한 임의의 필드(parameters, args 등)가 존재하는지 스캔해봅니다.
            if let Some(wrapped) = obj.get("tool_parameters").or(obj.get("parameters")).or(obj.get("arguments")).or(obj.get("args")) {
                // 필드 내부가 아예 맵 객체라면 그것을 가공하여 곧장 리턴에 사용합니다.
                if wrapped.is_object() {
                    // 성공값 반환
                    return Some((name_str, wrapped.clone()));
                } else if let Some(s) = wrapped.as_str() {
                    // 문자열 타입인 경우 혹시 내부적으로 텍스트 포맷의 JSON 데이터가 들어있는지 재차 분석을 돌려봅니다.
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
                        // 결과물이 정상적인 JSON 오브젝트라면 이를 그대로 리턴으로 뱉어줍니다.
                        if parsed.is_object() {
                            // 성공값 반환
                            return Some((name_str, parsed));
                        }
                    }
                }
            }
            
            // 만약 감싸진 하위 인자구조가 없는 플랫한 JSON 형태라면, name을 제외한 나머지 모든 필드들을 인자 맵으로 삼습니다.
            let mut params = serde_json::Map::new();
            // 맵 전체 필드를 순회합니다.
            for (k, v) in obj {
                // 키 이름이 name 혹은 tool_name이 아닌 순수 부가 변수들만 모읍니다.
                if k != "name" && k != "tool_name" {
                    // 인자 목록에 저장
                    params.insert(k.clone(), v.clone());
                }
            }
            // 가공 완료된 최종 도구 정보 묶음을 돌려줍니다.
            return Some((name_str, serde_json::Value::Object(params)));
        }
    }
    
    // 적절히 걸러진 양식이 매칭되지 않을 경우 무효(None) 리턴합니다.
    None
}

// 텍스트 전체에서 특수 구분태그(<|tool_call>) 및 임의 JSON 블록 파싱을 총동원하여 매칭되는 모든 도구 호출 목록을 배열로 확보합니다.
pub fn parse_all_custom_tool_calls(text: &str) -> Vec<(String, serde_json::Value)> {
    // 도구 이름과 인자 묶음을 담을 결과 리스트 컨테이너입니다.
    let mut results = Vec::new();
    
    // 텍스트 내부를 전진 검색하기 위한 탐색 인덱스 커서입니다.
    let mut search_pos = 0;
    // <|tool_call>call: 형식의 특수 태그가 나타나는 구간이 계속 발견되는 한 루프를 돌립니다.
    while let Some(start_pos) = text[search_pos..].find("<|tool_call>call:") {
        // 발견된 위치의 문자열 상 실제 절대 바이트 좌표를 구합니다.
        let actual_start = search_pos + start_pos;
        // 호출 접두사 토큰 문자열 길이 이후의 본문 텍스트 슬라이스를 잘라냅니다.
        let after_call = &text[actual_start + "<|tool_call>call:".len()..];
        // 함수 이름 뒤에 오는 최초의 여는 중괄호 '{' 위치를 찾아냅니다.
        if let Some(brace_pos) = after_call.find('{') {
            // 중괄호 직전 영역을 함수명으로 간주하여 공백 제거 후 잘라냅니다.
            let func_name = after_call[..brace_pos].trim().to_string();
            // 중괄호부터 뒤쪽 전부를 인자 JSON 영역 대상 후보로 설정합니다.
            let mut args_str = &after_call[brace_pos..];
            // 닫는 구분 지시 태그(<tool_call|>)가 포함되어 있다면 해당 표식 뒷부분을 모두 잘라냅니다.
            if let Some(end_pos) = args_str.find("<tool_call|>") {
                // 태그 앞부분까지만 유효한 파라미터 영역으로 축소
                args_str = &args_str[..end_pos];
            }
            // 텍스트 상에서 가장 마지막으로 닫히는 중괄호('}') 위치를 잡아냅니다.
            if let Some(last_brace) = args_str.rfind('}') {
                // 중괄호 바깥의 잔여 찌꺼기 텍스트 유입을 막기 위해 괄호 위치까지만 한정해서 자릅니다.
                args_str = &args_str[..=last_brace];
            }
            
            // 모델이 JSON 데이터 내부의 큰따옴표를 지시 토큰 형태(<|\"|>) 등으로 오염시켜 생성한 요소들을 복구 및 이스케이프 해제합니다.
            let mut cleaned_args = args_str
                .replace("<|\\\"|>", "\"")
                .replace("<|\"|>", "\"")
                .replace("\\\"", "\"")
                .trim()
                .to_string();
                
            // 혹시 데이터가 중복 중괄호({{...}}) 구조로 에워싸여 있으면 이를 한 꺼풀 벗겨 줍니다.
            while cleaned_args.starts_with("{{") && cleaned_args.ends_with("}}") {
                // 시작괄호 하나, 끝괄호 하나 제거
                cleaned_args = cleaned_args[1..cleaned_args.len()-1].trim().to_string();
            }
            
            // 확보한 인자 문자열을 최종적인 JSON 데이터 구조로 파싱해 봅니다.
            if let Ok(parsed_json) = serde_json::from_str::<serde_json::Value>(&cleaned_args) {
                // 파싱에 성공하면 함수 이름 및 JSON 매핑 데이터를 튜플 형태로 결과 배열에 탑재합니다.
                results.push((func_name, parsed_json));
            }
        }
        // 다음 검색 시에는 이번 탐색 기점 이후의 영역을 타깃 삼도록 오프셋 커서를 갱신해 줍니다.
        search_pos = actual_start + "<|tool_call>call:".len();
    }
    
    // 특수 지시 태그뿐만 아니라 텍스트 본문 한복판에 불쑥 튀어나오는 일반 JSON 블록 구문도 모두 조사합니다.
    let json_blocks = find_all_json_blocks(text);
    // 식별된 본문 속 개별 JSON 블록들을 정형성 분석기에 넣어봅니다.
    for (_, _, val) in json_blocks {
        // 도구 규격 포맷에 무사히 통과되어 도구명과 세부 인자를 식별한 경우
        if let Some((func_name, args_val)) = normalize_tool_call(&val) {
            // 이미 결과물 배열에 동일하게 포함되어 등록된 항목이 아닌 경우에만 신규로 리스트에 보충합니다.
            if !results.iter().any(|(name, args)| name == &func_name && args == &args_val) {
                // 최종 도구 튜플 보충
                results.push((func_name, args_val));
            }
        }
    }
    
    // 누적 확보된 모든 도구 호출 목록을 반환합니다.
    results
}

// 도구 실행 내역을 logs/tool_calls.log 파일에 영구 기록하는 로그 기록 유틸리티 함수입니다.
pub fn log_tool_call(name: &str, raw_args: &str, cleaned_args: &str, exit_code: i32, output: &str) {
    // 워크스페이스의 루트 경로를 알아옵니다.
    let root = get_workspace_root();
    // 로그 파일들이 밀집 저장될 logs 폴더의 전체 물리 경로를 정의합니다.
    let logs_dir = root.join("logs");
    // 해당 디렉토리가 미개발(부재) 상태인 경우 폴더를 자동 생성하며, 실패 시 에러 사유를 표준 에러에 보고합니다.
    if let Err(e) = fs::create_dir_all(&logs_dir) {
        // 에러 출력
        eprintln!("[시스템] [오류] 로그 디렉토리 생성 실패: {}", e);
    }
    // 실제로 작성할 로그 파일의 물리 경로를 logs/tool_calls.log 로 설정합니다.
    let log_file_path = logs_dir.join("tool_calls.log");
    // 로그 파일을 덧붙이기(append) 모드로 오픈하고 파일이 없을 시 새로 만듭니다.
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
    {
        // 파일 쓰기 스트림 기능(Write trait)을 가져옵니다.
        use std::io::Write;
        // 로그의 시작 구간을 구분하는 구분선 문장을 씁니다.
        let _ = writeln!(file, "========================================");
        // 기록을 수행하는 현재 실시간 ISO 8601 시각을 씁니다.
        let _ = writeln!(file, "시간: {}", get_iso8601_now());
        // 동작한 도구의 명칭을 기재합니다.
        let _ = writeln!(file, "도구명: {}", name);
        // 클라이언트로부터 접수된 가공 전 원래의 파라미터 묶음을 기입합니다.
        let _ = writeln!(file, "원본 인자: {}", raw_args);
        // 특수 태그가 삭제되어 정제 완료된 순수 인자 묶음을 기입합니다.
        let _ = writeln!(file, "정제된 인자: {}", cleaned_args);
        // 프로세스 종료 상태 코드를 기입합니다.
        let _ = writeln!(file, "종료 코드: {}", exit_code);
        // 도구 동작에 따른 표준출력 결과 및 Traceback 정보 등을 기록합니다.
        let _ = writeln!(file, "출력/결과:\n{}", output);
        // 로그 닫기 마감선을 씁니다.
        let _ = writeln!(file, "========================================");
    }
}

