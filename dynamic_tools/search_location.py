import sys
import json

def main():
    args_file = sys.argv[1]
    with open(args_file) as f:
        params = json.load(f)
    location_name = params.get('location_name')
    if location_name:
        # 실제 검색 로직이 들어갈 자리입니다. 여기서는 예시로 응답합니다.
        if location_name == '김천':
            print('김천은 경상북도 김천군에 위치하고 있습니다.')
        else:
            print(f'{location_name}에 대한 정보를 찾을 수 없습니다.')
    else:
        print('검색할 위치 이름이 제공되지 않았습니다.')

main()