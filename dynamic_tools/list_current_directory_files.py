import os

def list_large_files(min_size_kb=10):
    min_size_bytes = min_size_kb * 1024
    print(f"--- 현재 환경에서 크기가 {min_size_kb}KB 이상인 파일 목록 ---")
    found_files = False
    try:
        for filename in os.listdir('.'):
            file_path = os.path.join('.', filename)
            if os.path.isfile(file_path):
                try:
                    file_size_bytes = os.path.getsize(file_path)
                    if file_size_bytes >= min_size_bytes:
                        file_size_kb = file_size_bytes / 1024
                        print(f"파일 이름: {filename:<30} | 크기: {file_size_kb:.2f} KB")
                        found_files = True
                except OSError as e:
                    print(f"파일 정보 읽기 오류 ({filename}): {e}")
    except Exception as e:
        print(f"오류 발생: {e}")
    if not found_files:
        print(f"지정된 크기({min_size_kb}KB) 이상의 파일이 없습니다.")

# 도구 실행 시 기본값으로 10KB 이상을 확인합니다.
list_large_files(min_size_kb=10)