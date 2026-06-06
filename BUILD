cc_binary(
    name = "multimodal_server",
    srcs = [
        "main.cpp",
        "httplib.h",
        "json.hpp",
    ],
    data = select({
        "@platforms//os:linux": [
            "@litert_lm//prebuilt/linux_x86_64:libLiteRtWebGpuAccelerator.so",
        ],
        "//conditions:default": [],
    }),
    linkopts = ["-lpthread"],
    deps = [
        "@litert_lm//c:engine",
    ],
)

