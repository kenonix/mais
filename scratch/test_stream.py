import requests
import json

url = "http://127.0.0.1:11435/api/chat"
payload = {
    "messages": [
        {"role": "user", "content": "한국 경제 동향을 알려줘"}
    ],
    "stream": True
}

response = requests.post(url, json=payload, stream=True)
with open("/home/kenonix/gits/mais/scratch/raw_chunks.txt", "w") as f:
    for line in response.iter_lines():
        if line:
            decoded = line.decode('utf-8')
            f.write(decoded + "\n")
            print(decoded[:100])
