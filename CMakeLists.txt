cmake_minimum_required(VERSION 3.16)
project(LiteRT_LM_Multimodal_CLI VERSION 1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED True)

# LiteRT-LM 경로 설정
set(LITERT_LM_ROOT "${CMAKE_SOURCE_DIR}/LiteRT-LM")
include_directories(${LITERT_LM_ROOT}) # 'c/engine.h'를 찾기 위한 루트 경로

# 라이브러리 경로 (Linux x86_64 prebuilt)
set(LITERT_LIB_DIR "${LITERT_LM_ROOT}/prebuilt/linux_x86_64")
link_directories(${LITERT_LIB_DIR})

add_executable(multimodal_cli main.cpp)

# 의존성 링크: libLiteRt.so 하나만 링크하면 됩니다.
target_link_libraries(multimodal_cli
    PRIVATE
    LiteRt    # libLiteRt.so 를 의미함
    pthread
    dl
)
