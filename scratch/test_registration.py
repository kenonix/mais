#!/usr/bin/env python3
import urllib.request
import json
import sys

def main():
    url = "http://localhost:11435/v1/chat/completions"
    
    # 1. Register a new dynamic tool named "count_files"
    code = """import sys
import json
import os

with open(sys.argv[1], 'r') as f:
    args = json.load(f)

path = args.get("path", ".")
try:
    files = os.listdir(path)
    count = len([f for f in files if os.path.isfile(os.path.join(path, f))])
    print(json.dumps({"status": "success", "count": count, "files": files[:5]}))
except Exception as e:
    print(json.dumps({"status": "error", "message": str(e)}))
"""

    data = {
        "model": "litert-lm:latest",
        "messages": [
            {
                "role": "user",
                "content": "이 요청은 직접 도구를 테스트하는 요청입니다."
            }
        ],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "create_or_update_tool",
                    "description": "새로운 동적 도구를 등록합니다.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "description": {"type": "string"},
                            "parameters": {"type": "object"},
                            "code": {"type": "string"}
                        },
                        "required": ["name", "description", "parameters", "code"]
                    }
                }
            }
        ],
        "stream": False
    }

    # Let's bypass the model and just directly invoke the C++ backend ExecuteTool if possible,
    # or simulate the model calling the tool!
    # Wait, the C++ backend doesn't expose a direct tool execution endpoint, 
    # but we can send a chat request where the model is forced/prompted to use the tool,
    # or we can test if the tool is loaded.
    # Actually, let's send a chat request where the user prompt is:
    # "create_or_update_tool 도구를 사용하여, 이름 'count_files', 설명 '지정된 경로의 파일 개수 세기', 파라미터는 path(string, optional)를 가지는 도구를 등록해 줘."
    
    prompt = "create_or_update_tool 도구를 사용하여 다음 도구를 등록해줘.\n이름: count_files\n설명: 지정된 경로의 파일 개수 세기\n파라미터: properties에 path(string 타입, 파일 개수를 셀 디렉토리 경로, 선택사항)를 포함한 규격\n코드:\n" + code

    data["messages"] = [{"role": "user", "content": prompt}]
    
    req = urllib.request.Request(
        url,
        data=json.dumps(data).encode("utf-8"),
        headers={"Content-Type": "application/json"}
    )

    print("Sending registration request to server...")
    try:
        with urllib.request.urlopen(req) as response:
            res_body = response.read().decode("utf-8")
            print("Response:")
            print(json.dumps(json.loads(res_body), indent=2, ensure_ascii=False))
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)

if __name__ == "__main__":
    main()
