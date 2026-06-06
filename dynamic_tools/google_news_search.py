import requests
import json
import sys
<<<<<<< HEAD
import os

try:
    from dotenv import load_dotenv
    load_dotenv()
except ImportError:
    pass

API_KEY = os.environ.get("NEWSAPI_KEY", "")
=======

# API 키는 환경 변수에서 가져오거나, 이 경우 직접 코드에 포함합니다.
# 실제 운영 환경에서는 환경 변수 사용을 강력히 권장합니다.
API_KEY = "d64e44d9a53a4b85a75d0424b836f7fd"
>>>>>>> d369123 (여러가지 추가)
BASE_URL = "https://newsapi.org/v2/everything"

def search_news(query, language):
    """
    NewsAPI를 사용하여 지정된 키워드로 뉴스를 검색합니다.
    """
    params = {
        "q": query,
        "language": language,
        "sortBy": "publishedAt",
        "apiKey": API_KEY
    }
    
    try:
        response = requests.get(BASE_URL, params=params)
        response.raise_for_status()  # HTTP 오류 발생 시 예외 발생
        data = response.json()
        
        if data.get("status") == "ok":
            articles = data.get("articles", [])
            if not articles:
                return "검색 결과가 없습니다. 키워드나 언어를 확인해 주세요."
            
            results = []
            for i, article in enumerate(articles[:5]): # 상위 5개 결과만 출력
                title = article.get("title", "제목 없음")
                description = article.get("description", "설명 없음")
                url = article.get("url", "URL 없음")
                source = article.get("source", {}).get("name", "출처 없음")
                
                results.append({
                    "순위": i + 1,
                    "제목": title,
                    "출처": source,
                    "링크": url
                })
            
            return json.dumps(results, indent=2, ensure_ascii=False)
        else:
            return f"API 오류 발생: {data.get('message', '알 수 없는 오류')}"

    except requests.exceptions.RequestException as e:
        return f"네트워크 요청 중 오류가 발생했습니다: {e}"
    except Exception as e:
        return f"처리 중 예외가 발생했습니다: {e}"

if __name__ == "__main__":
    # sys.argv[1]은 입력 JSON 파일 경로를 나타냅니다.
    try:
        with open(sys.argv[1], 'r', encoding='utf-8') as f:
            args = json.load(f)
        
        query = args.get("query", "Palestine War")
        language = args.get("language", "en")
        
        result = search_news(query, language)
        print(result)
        
    except FileNotFoundError:
        print("오류: 입력 파일 경로를 찾을 수 없습니다.")
    except json.JSONDecodeError:
        print("오류: 입력 파일이 올바른 JSON 형식이 아닙니다.")
    except Exception as e:
        print(f"스크립트 실행 중 오류 발생: {e}")