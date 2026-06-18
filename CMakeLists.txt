cmake_minimum_required(VERSION 3.16)
project(LiteRT_LM_Multimodal_CLI VERSION 1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED True)

# LiteRT-LM 경로 설정
set(LITERT_LM_ROOT "${CMAKE_SOURCE_DIR}/bazel-mais/external/litert_lm" CACHE PATH "LiteRT-LM source root")
set(LITERT_LM_SYS_LIB_DIR "$ENV{HOME}/.cache/litert-lm-sys/v0.10.2/x86_64-unknown-linux-gnu" CACHE PATH "litert-lm-sys native library dir")
set(LITERT_SYS_LIB_DIR "$ENV{HOME}/.cache/litert-sys/v0.10.2/x86_64-unknown-linux-gnu" CACHE PATH "litert-sys native library dir")

if(NOT EXISTS "${LITERT_LM_ROOT}/c/engine.h")
    message(FATAL_ERROR "LiteRT-LM C header not found: ${LITERT_LM_ROOT}/c/engine.h")
endif()
if(NOT EXISTS "${LITERT_LM_SYS_LIB_DIR}/libLiteRtLmC.so")
    message(FATAL_ERROR "libLiteRtLmC.so not found: ${LITERT_LM_SYS_LIB_DIR}")
endif()
if(NOT EXISTS "${LITERT_SYS_LIB_DIR}/libLiteRt.so")
    message(FATAL_ERROR "libLiteRt.so not found: ${LITERT_SYS_LIB_DIR}")
endif()

include_directories(${LITERT_LM_ROOT}) # 'c/engine.h'를 찾기 위한 루트 경로

# 라이브러리 경로 (Linux x86_64 prebuilt)
link_directories(${LITERT_LM_SYS_LIB_DIR} ${LITERT_SYS_LIB_DIR})

add_executable(multimodal_cli main.cpp)

target_link_libraries(multimodal_cli
    PRIVATE
    LiteRtLmC
    LiteRt
    GemmaModelConstraintProvider
    pthread
    dl
)

set_target_properties(multimodal_cli PROPERTIES
    BUILD_RPATH "${LITERT_LM_SYS_LIB_DIR};${LITERT_SYS_LIB_DIR};$ORIGIN"
    INSTALL_RPATH "${LITERT_LM_SYS_LIB_DIR};${LITERT_SYS_LIB_DIR};$ORIGIN"
)

foreach(gpu_lib
    libLiteRtWebGpuAccelerator.so
    libLiteRtTopKWebGpuSampler.so
)
    if(EXISTS "${LITERT_SYS_LIB_DIR}/${gpu_lib}")
        add_custom_command(TARGET multimodal_cli POST_BUILD
            COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${LITERT_SYS_LIB_DIR}/${gpu_lib}"
                "$<TARGET_FILE_DIR:multimodal_cli>/${gpu_lib}"
        )
    endif()
endforeach()
