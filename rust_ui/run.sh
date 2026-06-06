#!/bin/bash
# 혜연 AI 채팅 클라이언트 실행 스크립트
# 사용법: ./run.sh [--port PORT] [--host HOST]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$SCRIPT_DIR/target/release/rust_ui"

# 바이너리가 없으면 빌드
if [ ! -f "$BINARY" ]; then
    echo "🔨 첫 실행: 바이너리를 빌드합니다..."
    cargo build --release --manifest-path="$SCRIPT_DIR/Cargo.toml"
fi

exec "$BINARY" "$@"
