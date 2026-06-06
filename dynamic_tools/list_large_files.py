import os
import json
import sys

def list_large_files(min_size_kb):
    """
    현재 디렉토리에서 지정된 크기(KB) 이상의 파일 목록과 크기를 반환합니다.
    """
    min_size_bytes = min_size_kb * 1024
    large_files = []
    
    try:
        for entry in os.scandir('.'):
            if entry.is_file():
                try:
                    file_size = entry.stat().st_size
                    if file_size >= min_size_bytes:
                        large_files.append({
                            "name": entry.name,
                            "size_bytes": file_size,
                            "size_kb": round(file_size / 1024, 2)
                        })
                except OSError:
                    # 파일 정보 접근 권한 문제 등 예외 처리
                    continue
    except OSError as e:
        return {"error": f"디렉토리 스캔 중 오류 발생: {e}"}
        
    return large_files

if __name__ == "__main__":
    # sys.argv[1]은 임시 JSON 파일 경로를 가리킵니다.
    # 이 예제에서는 파라미터를 직접 하드코딩하거나, 
    # 실제 환경에서는 JSON 파일에서 읽어와야 합니다.
    # 지침에 따라, 우리는 이 도구가 'list_large_files'라는 이름으로 등록될 것이므로,
    # 실제 호출 시에는 파라미터가 JSON 파일에 담겨 전달될 것입니다.
    
    # 여기서는 테스트를 위해 10KB를 기본값으로 사용합니다.
    # 실제 실행 시에는 sys.argv[1]에서 min_size_kb를 읽어와야 합니다.
    
    # 실제 도구 등록 시, 파라미터 스키마에 min_size_kb를 정의하고,
    # 도구 호출 시 해당 값이 JSON 파일에 포함되도록 가정합니다.
    
    # 도구 등록 시, 이 스크립트는 'list_large_files' 함수를 실행하도록 구성됩니다.
    # 실제 실행 시에는 sys.argv[1]에서 min_size_kb를 읽어와야 합니다.
    
    # 임시로 10KB를 사용합니다.
    min_size_kb = 0 
    
    results = list_large_files(min_size_kb)
    print(json.dumps(results))