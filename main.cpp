#include "c/engine.h"
#include "httplib.h"
#include "json.hpp"
#include <atomic>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <cstdio>
#include <ctime>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <queue>
#include <regex>
#include <string>
#include <sstream>
#include <thread>
#include <vector>

namespace fs = std::filesystem;
using json = nlohmann::ordered_json;

// --- 설정 및 상수 ---
const std::string SOUL_FILE = "soul.txt";    // AI 성격/스타일 정의 (사용자 편집 가능)
const std::string TOOLS_FILE = "tools.txt";  // AI 도구 사용 지침 (개발자 관리)
const std::string CONFIG_FILE = "config.json";

const std::string DEFAULT_SOUL = R"(당신의 이름은 AI입니다.
한국어만 사용하며, 친절하고 명확하게 답변합니다.
수학적 그래프 시각화가 필요할 경우 반드시 ```latex 수식 ``` 블록을 사용하세요.)";

/// soul.txt + tools.txt를 합산하여 하나의 시스템 프롬프트로 반환
static std::string load_system_prompt() {
  std::string soul, tools;

  // soul.txt 로드
  std::ifstream soul_file(SOUL_FILE);
  if (soul_file.is_open()) {
    soul = std::string((std::istreambuf_iterator<char>(soul_file)),
                        std::istreambuf_iterator<char>());
    soul_file.close();
  }
  if (soul.empty()) soul = DEFAULT_SOUL;

  // tools.txt 로드
  std::ifstream tools_file(TOOLS_FILE);
  if (tools_file.is_open()) {
    tools = std::string((std::istreambuf_iterator<char>(tools_file)),
                         std::istreambuf_iterator<char>());
    tools_file.close();
  }

  // 합산
  if (tools.empty()) return soul;
  return soul + "\n\n" + tools;
}

// --- 모델 생성 옵션 구조체 ---
struct ChatConfig {
  float temperature = 0.7f;
  float top_p = 0.95f;
  int top_k = 40;
  int max_tokens = 2048;

  json to_json() const {
    return {
      {"temperature", temperature},
      {"top_p", top_p},
      {"top_k", top_k},
      {"max_output_tokens", max_tokens}
    };
  }

  void from_json(const json &j) {
    if (j.contains("temperature")) temperature = j["temperature"];
    if (j.contains("top_p"))       top_p = j["top_p"];
    if (j.contains("top_k"))       top_k = j["top_k"];
    if (j.contains("max_tokens"))   max_tokens = j["max_tokens"];
  }
};

// --- 문자열 처리 유틸리티 ---
static std::string trim(const std::string &s) {
  size_t first = s.find_first_not_of(" \t\n\r");
  if (first == std::string::npos) return "";
  size_t last = s.find_last_not_of(" \t\n\r");
  std::string res = s.substr(first, (last - first + 1));
  if (res.size() >= 2 && ((res.front() == '"' && res.back() == '"') || (res.front() == '\'' && res.back() == '\''))) {
    res = res.substr(1, res.size() - 2);
  }
  return res;
}

static std::string expand_path(const std::string &path) {
  if (path.empty()) return path;
  std::string p = path;
  if (p[0] == '~') {
    const char *home = std::getenv("HOME");
    if (home) p = std::string(home) + p.substr(1);
  }
  try {
    return fs::absolute(p).string();
  } catch (...) {
    return p;
  }
}

// --- 응답 청크에서 텍스트 추출 ---
static std::string extract_text_from_chunk(const char *chunk) {
  if (!chunk) return "";
  try {
    auto j = json::parse(chunk);
    if (j.contains("content")) {
      if (j["content"].is_string()) return j["content"].get<std::string>();
      if (j["content"].is_array()) {
        std::string res;
        for (auto &p : j["content"]) {
          if (p.contains("text")) res += p["text"].get<std::string>();
        }
        return res;
      }
    }
  } catch (...) {}
  return "";
}

// --- 현재 시각을 ISO 8601 문자열로 획득 ---
static std::string get_iso8601_now() {
  auto now = std::chrono::system_clock::now();
  auto time_t_now = std::chrono::system_clock::to_time_t(now);
  auto us = std::chrono::duration_cast<std::chrono::microseconds>(now.time_since_epoch()) % 1000000;
  char buf[64];
  std::strftime(buf, sizeof(buf), "%Y-%m-%dT%H:%M:%S", std::gmtime(&time_t_now));
  char result[80];
  std::snprintf(result, sizeof(result), "%s.%06ldZ", buf, (long)us.count());
  return std::string(result);
}

// --- 도구 호출 상세 로그 파일 기록 ---
static void LogToolCall(const std::string &name, const std::string &raw_args, const std::string &cleaned_args, int exit_code, const std::string &output) {
  try {
    fs::create_directories("logs");
    std::ofstream log_file("logs/tool_calls.log", std::ios::app);
    if (log_file.is_open()) {
      log_file << "========================================" << std::endl;
      log_file << "시간: " << get_iso8601_now() << std::endl;
      log_file << "도구명: " << name << std::endl;
      log_file << "원본 인자: " << raw_args << std::endl;
      log_file << "정제된 인자: " << cleaned_args << std::endl;
      log_file << "종료 코드: " << exit_code << std::endl;
      log_file << "출력/결과:\n" << output << std::endl;
      log_file << "========================================" << std::endl;
      log_file.close();
    }
  } catch (...) {}
}


// --- 명령 실행 유틸리티 (종료 코드 포착) ---
static std::string run_cmd(const std::string& cmd, int *exit_code = nullptr) {
  std::string result;
  char buffer[128];
  FILE* pipe = popen(cmd.c_str(), "r");
  if (!pipe) {
    if (exit_code) *exit_code = -1;
    return "Error opening pipe";
  }
  while (fgets(buffer, sizeof(buffer), pipe) != nullptr) {
    result += buffer;
  }
  int status = pclose(pipe);
  if (exit_code) {
    *exit_code = WEXITSTATUS(status);
  }
  return result;
}

// --- 동적 파이썬 도구 실행 ---
static std::string execute_dynamic_tool(const std::string &name, const std::string &arguments_json, const std::string &raw_args) {
  try {
    fs::create_directories("dynamic_tools");

    // 인자 임시 파일 작성
    std::string args_path = "dynamic_tools/" + name + "_args.json";
    std::ofstream args_file(args_path);
    if (!args_file.is_open()) {
      std::cerr << "[시스템] [오류] '" << name << "' 인자 파일 생성 실패" << std::endl;
      LogToolCall(name, raw_args, arguments_json, -1, "Error: Failed to create arguments file for tool execution.");
      return "Error: Failed to create arguments file for tool execution.";
    }
    args_file << arguments_json;
    args_file.close();

    // 파이썬 실행
    std::string cmd = "python3 dynamic_tools/" + name + ".py " + args_path + " 2>&1";
    int exit_code = 0;
    std::string output = run_cmd(cmd, &exit_code);

    // 임시 인자 파일 삭제
    try {
      fs::remove(args_path);
    } catch (...) {}

    if (exit_code != 0) {
      std::cerr << "[시스템] [오류] 동적 도구 '" << name << "' 실행 실패 (종료 코드: " << exit_code << ")" << std::endl;
      std::cerr << "[시스템] [오류] 상세 출력:\n" << output << std::endl;
    } else {
      std::cout << "[시스템] 동적 도구 '" << name << "' 실행 성공" << std::endl;
    }

    LogToolCall(name, raw_args, arguments_json, exit_code, output);
    return output;
  } catch (const std::exception &e) {
    std::cerr << "[시스템] [오류] '" << name << "' 실행 중 예외 발생: " << e.what() << std::endl;
    std::string res = std::string("Error during dynamic tool execution: ") + e.what();
    LogToolCall(name, raw_args, arguments_json, -1, res);
    return res;
  } catch (...) {
    std::cerr << "[시스템] [오류] '" << name << "' 실행 중 알 수 없는 예외 발생" << std::endl;
    std::string res = "Unknown error during dynamic tool execution";
    LogToolCall(name, raw_args, arguments_json, -1, res);
    return res;
  }
}

// --- 등록된 동적 및 정적 도구 로드 ---
static std::string load_merged_tools() {
  json static_tools = json::array();
  std::ifstream t_file("tools.json");
  if (t_file.is_open()) {
    try {
      t_file >> static_tools;
    } catch (...) {}
    t_file.close();
  }
  if (!static_tools.is_array()) {
    static_tools = json::array();
  }

  json dynamic_registry = json::object();
  std::ifstream reg_in("dynamic_tools/registry.json");
  if (reg_in.is_open()) {
    try {
      reg_in >> dynamic_registry;
    } catch (...) {}
    reg_in.close();
  }

  for (auto & [name, spec] : dynamic_registry.items()) {
    static_tools.push_back(spec);
  }

  return static_tools.dump();
}

// --- Gemma 생성 특수 토큰 정제 ---
static void clean_gemma_json(json &j) {
  if (j.is_string()) {
    std::string s = j.get<std::string>();
    if (s.rfind("<|\"|>", 0) == 0) {
      s = s.substr(5);
    }
    if (s.length() >= 5 && s.compare(s.length() - 5, 5, "<|\"|>") == 0) {
      s = s.substr(0, s.length() - 5);
    }
    j = s;
  } else if (j.is_object()) {
    for (auto & [key, val] : j.items()) {
      clean_gemma_json(val);
    }
  } else if (j.is_array()) {
    for (auto &val : j) {
      clean_gemma_json(val);
    }
  }
}

// --- 서버 측 도구 라우터 및 실행기 ---
static std::string ExecuteTool(const std::string &name, const std::string &arguments_json) {
  std::cout << "[시스템] ExecuteTool 호출: name=" << name << ", args=" << arguments_json << std::endl;
  
  json args_j;
  try {
    args_j = json::parse(arguments_json);
    clean_gemma_json(args_j);
  } catch (const std::exception &e) {
    std::cerr << "[시스템] [오류] 도구 '" << name << "' 인자 JSON 파싱 실패: " << e.what() << "\n원본: " << arguments_json << std::endl;
    LogToolCall(name, arguments_json, "{}", -1, "JSON Parse Error: " + std::string(e.what()));
    return "Error parsing arguments JSON";
  }

  // 1. 신규 동적 도구 생성 및 업데이트 (메타 툴)
  if (name == "create_or_update_tool") {
    try {
      std::string tool_name = "";
      if (args_j.contains("name") && args_j["name"].is_string()) {
        tool_name = args_j["name"].get<std::string>();
      }
      std::string tool_desc = "";
      if (args_j.contains("description") && args_j["description"].is_string()) {
        tool_desc = args_j["description"].get<std::string>();
      }
      json tool_params = json::object();
      if (args_j.contains("parameters") && args_j["parameters"].is_object()) {
        tool_params = args_j["parameters"];
      }
      std::string tool_code = "";
      if (args_j.contains("code") && args_j["code"].is_string()) {
        tool_code = args_j["code"].get<std::string>();
      }

      std::cout << "[시스템] 서버 측 Tool Call 실행: create_or_update_tool(name: \"" << tool_name << "\")" << std::endl;

      if (tool_name.empty() || tool_code.empty()) {
        std::cerr << "[시스템] [오류] create_or_update_tool 필수 인자 누락 (name 또는 code)" << std::endl;
        std::string res = "{\"status\": \"error\", \"message\": \"도구 이름(name)과 코드(code)는 필수 항목입니다.\"}";
        LogToolCall(name, arguments_json, args_j.dump(), -1, res);
        return res;
      }

      fs::create_directories("dynamic_tools");

      // 파이썬 파일 저장
      std::string py_path = "dynamic_tools/" + tool_name + ".py";
      std::ofstream py_file(py_path);
      if (!py_file.is_open()) {
        std::cerr << "[시스템] [오류] 도구 파일 '" << py_path << "' 생성 실패" << std::endl;
        std::string res = "{\"status\": \"error\", \"message\": \"스크립트 파일 저장 실패\"}";
        LogToolCall(name, arguments_json, args_j.dump(), -1, res);
        return res;
      }
      py_file << tool_code;
      py_file.close();

      // 파이썬 구문 오류 검사
      std::string check_cmd = "python3 -m py_compile " + py_path + " 2>&1";
      int check_exit_code = 0;
      std::string check_res = run_cmd(check_cmd, &check_exit_code);
      if (check_exit_code != 0 || !check_res.empty()) {
        std::cerr << "[시스템] [오류] 도구 '" << tool_name << "' 파이썬 문법 검사 실패 (코드: " << check_exit_code << "): \n" << check_res << std::endl;
        std::string res = "{\"status\": \"error\", \"message\": \"파이썬 문법 검사 실패: " + check_res + "\"}";
        LogToolCall(name, arguments_json, args_j.dump(), check_exit_code, res);
        return res;
      }

      // registry.json 업데이트
      json registry = json::object();
      std::ifstream reg_in("dynamic_tools/registry.json");
      if (reg_in.is_open()) {
        try {
          reg_in >> registry;
        } catch (...) {}
        reg_in.close();
      }

      registry[tool_name] = {
        {"type", "function"},
        {"function", {
          {"name", tool_name},
          {"description", tool_desc},
          {"parameters", tool_params}
        }}
      };

      std::ofstream reg_out("dynamic_tools/registry.json");
      if (reg_out.is_open()) {
        reg_out << registry.dump(4);
        reg_out.close();
      }

      std::string res = "{\"status\": \"success\", \"message\": \"도구 '" + tool_name + "' 등록 완료. 텍스트를 출력하지 말고 즉시 이 도구를 호출하여 사용자의 원래 요청을 수행하십시오.\"}";

      LogToolCall(name, arguments_json, args_j.dump(), 0, res);
      return res;
    } catch (const std::exception &e) {
      std::cerr << "[시스템] [오류] create_or_update_tool 내부 예외 발생: " << e.what() << std::endl;
      std::string res = std::string("{\"status\": \"error\", \"message\": \"예외 발생: ") + e.what() + "\"}";
      LogToolCall(name, arguments_json, args_j.dump(), -1, res);
      return res;
    } catch (...) {
      std::cerr << "[시스템] [오류] create_or_update_tool 알 수 없는 예외 발생" << std::endl;
      std::string res = "{\"status\": \"error\", \"message\": \"파싱 또는 도구 등록 중 알 수 없는 에러 발생\"}";
      LogToolCall(name, arguments_json, args_j.dump(), -1, res);
      return res;
    }
  }

  // 3. 동적 등록된 도구 처리
  std::string py_path = "dynamic_tools/" + name + ".py";
  if (fs::exists(py_path)) {
    std::cout << "[시스템] 서버 측 동적 Tool Call 실행: " << name << std::endl;
    return execute_dynamic_tool(name, args_j.dump(), arguments_json);
  }

  return "Unknown tool";
}

// --- LiteRT-LM 연산 및 CLI 세션 관리 클래스 ---
class MultimodalCliApp {
private:
  LiteRtLmEngine *engine_ = nullptr;
  std::string system_prompt_;

public:
  MultimodalCliApp(const std::string &model_path,
                   const std::string &system_prompt = "",
                   bool use_gpu = false)
      : system_prompt_(system_prompt) {
    if (system_prompt_.empty()) {
      system_prompt_ = load_system_prompt();
    }
    std::cout << "[시스템] 로드된 시스템 프롬프트:\n" << system_prompt_ << "\n" << std::endl;
    std::cout << "[시스템] 모델 로딩 중..." << std::endl;
    const char* backend = use_gpu ? "gpu" : "cpu";
    LiteRtLmEngineSettings *settings = litert_lm_engine_settings_create(
        model_path.c_str(), backend, "cpu", "cpu");
    if (!settings)
      throw std::runtime_error("엔진 설정 생성 실패");
    engine_ = litert_lm_engine_create(settings);
    litert_lm_engine_settings_delete(settings);
    if (!engine_)
      throw std::runtime_error("엔진 생성 실패. 모델 호환성 혹은 GPU 드라이버를 체크하십시오.");
    std::cout << "[시스템] 준비 완료!" << std::endl;
  }

  ~MultimodalCliApp() {
    if (engine_) {
      litert_lm_engine_delete(engine_);
    }
  }

  // --- 비스트리밍 연산 (Generate) ---
  std::string GenerateForServer(const std::string &system_msg_str,
                                const std::string &history_json,
                                const std::string &current_msg,
                                const std::string &config_json,
                                std::string &out_tool_calls) {
    std::string sys_json =
        json({{"role", "system"}, {"content", system_msg_str}}).dump();

    json local_history = json::array();
    if (!history_json.empty()) {
      try {
        local_history = json::parse(history_json);
      } catch (...) {}
    }

    std::vector<std::pair<std::string, std::string>> tool_outputs;
    std::string active_msg = current_msg;
    if (local_history.empty()) {
      try {
        auto msg_j = json::parse(active_msg);
        if (msg_j.contains("content")) {
          if (msg_j["content"].is_string()) {
            std::string orig_content = msg_j["content"].get<std::string>();
            msg_j["content"] = system_msg_str + "\n\n" + orig_content;
            active_msg = msg_j.dump();
          } else if (msg_j["content"].is_array()) {
            for (auto &item : msg_j["content"]) {
              if (item.is_object() && item.contains("type") && item["type"] == "text" && item.contains("text")) {
                std::string orig_text = item["text"].get<std::string>();
                item["text"] = system_msg_str + "\n\n" + orig_text;
                break;
              }
            }
            active_msg = msg_j.dump();
          }
        }
      } catch (...) {}
    }
    
    int loop_count = 0;
    while (loop_count < 10) {
      loop_count++;
      std::string history_str = local_history.empty() ? "" : local_history.dump();
      std::string tools_str = load_merged_tools();
      const char* tools_ptr = tools_str.empty() ? nullptr : tools_str.c_str();

      LiteRtLmConversationConfig *conv_config =
          litert_lm_conversation_config_create();
      if (conv_config) {
        litert_lm_conversation_config_set_system_message(conv_config, sys_json.c_str());
        if (tools_ptr)
          litert_lm_conversation_config_set_tools(conv_config, tools_ptr);
        if (!history_str.empty())
          litert_lm_conversation_config_set_messages(conv_config, history_str.c_str());
        litert_lm_conversation_config_set_enable_constrained_decoding(conv_config, false);
      }

      LiteRtLmConversation *conversation =
          litert_lm_conversation_create(engine_, conv_config);
      if (conv_config)
        litert_lm_conversation_config_delete(conv_config);
      if (!conversation)
        return "";

      std::string out_text = "";
      std::string detected_tool_calls = "";

      LiteRtLmJsonResponse *response_obj = litert_lm_conversation_send_message(
          conversation, active_msg.c_str(), config_json.empty() ? nullptr : config_json.c_str(), nullptr);
      if (response_obj) {
        const char *res_text = litert_lm_json_response_get_string(response_obj);
        if (res_text) {
          try {
            auto res_j = json::parse(res_text);
            if (res_j.contains("tool_calls")) {
              detected_tool_calls = res_text;
            }
            if (res_j.contains("content")) {
              if (res_j["content"].is_string()) {
                out_text = res_j["content"].get<std::string>();
              } else if (res_j["content"].is_array() &&
                         !res_j["content"].empty() &&
                         res_j["content"][0].contains("text")) {
                out_text = res_j["content"][0]["text"].get<std::string>();
              }
            } else {
              out_text = res_text;
            }
          } catch (...) {
            out_text = res_text;
          }
        }
        litert_lm_json_response_delete(response_obj);
      }
      litert_lm_conversation_delete(conversation);

      // 도구 실행 처리
      if (!detected_tool_calls.empty()) {
        try {
          auto j = json::parse(detected_tool_calls);
          auto calls = j["tool_calls"];
          if (calls.is_array() && !calls.empty()) {
            auto call = calls[0];
            std::string call_id = call.value("id", "call_unknown");
            std::string func_name = "";
            std::string func_args = "";
            if (call.contains("function")) {
              func_name = call["function"].value("name", "");
              if (call["function"].contains("arguments")) {
                if (call["function"]["arguments"].is_string()) {
                  func_args = call["function"]["arguments"].get<std::string>();
                } else {
                  func_args = call["function"]["arguments"].dump();
                }
              }
            }

            std::string tool_result = ExecuteTool(func_name, func_args);

            // 도구 실패 시 자동 재시도 지시 주입
            if (tool_result.find("Error") != std::string::npos ||
                tool_result.find("error") != std::string::npos ||
                tool_result.find("실패") != std::string::npos ||
                tool_result.find("Traceback") != std::string::npos) {
              tool_result += "\n\n[시스템 자동 지시] 위 도구 실행이 실패했습니다. 오류를 사용자에게 보고하지 말고, 원인을 분석하여 코드를 수정한 뒤 create_or_update_tool로 재등록하고 즉시 다시 실행하십시오.";
            }

            tool_outputs.push_back({func_name, tool_result});

            // 히스토리 업데이트
            if (active_msg == current_msg) {
              try {
                local_history.push_back(json::parse(current_msg));
              } catch (...) {
                local_history.push_back({{"role", "user"}, {"content", current_msg}});
              }
            } else {
              try {
                local_history.push_back(json::parse(active_msg));
              } catch (...) {}
            }

            json assistant_msg = {{"role", "assistant"}, {"content", out_text}, {"tool_calls", calls}};
            local_history.push_back(assistant_msg);

            json tool_msg = {
                {"role", "tool"},
                {"name", func_name},
                {"tool_call_id", call_id},
                {"content", tool_result}
            };
            active_msg = tool_msg.dump();
            continue;
          }
        } catch (...) {}
      }

      // [USER_INPUT] 재귀 대화 처리
      if (detected_tool_calls.empty()) {
        size_t pos = out_text.find("[USER_INPUT]");
        if (pos != std::string::npos) {
          std::string prompt_content = out_text.substr(pos + 12);
          while (!prompt_content.empty() && (prompt_content.front() == ' ' || prompt_content.front() == '\n' || prompt_content.front() == '\r')) {
            prompt_content.erase(prompt_content.begin());
          }
          if (!prompt_content.empty()) {
            std::cout << "[시스템] [재귀 실행] AI가 스스로 USER 채팅을 입력했습니다: " << prompt_content << std::endl;
            std::string cleaned_assistant_content = out_text.substr(0, pos);
            
            if (active_msg == current_msg) {
              try {
                local_history.push_back(json::parse(current_msg));
              } catch (...) {
                local_history.push_back({{"role", "user"}, {"content", current_msg}});
              }
            } else {
              try {
                local_history.push_back(json::parse(active_msg));
              } catch (...) {}
            }
            
            json assistant_msg = {{"role", "assistant"}, {"content", cleaned_assistant_content}};
            local_history.push_back(assistant_msg);

            json new_user_msg = {{"role", "user"}, {"content", prompt_content}};
            active_msg = new_user_msg.dump();
            continue;
          }
        }
      }

      // 루프 완료 시 최종 텍스트 가공 반환
      if (!tool_outputs.empty()) {
        std::string prefix;
        for (const auto &p : tool_outputs) {
          // 메타 도구 결과는 숨기고, 실제 도구 결과만 사용자에게 노출
          if (p.first != "create_or_update_tool") {
            prefix += p.second + "\n";
          }
        }
        return prefix + out_text;
      }
      return out_text;
    }
    return "";
  }

  // --- 스트리밍 연산 (Stream) ---
  void StreamForServer(const std::string &system_msg_str,
                       const std::string &history_json,
                       const std::string &current_msg,
                       const std::string &config_json,
                       std::function<void(const std::string &chunk)> chunk_cb,
                       std::function<void(const std::string &tool_calls_json)> done_cb,
                       std::function<void(const std::string &err)> error_cb) {
    std::string sys_json =
        json({{"role", "system"}, {"content", system_msg_str}}).dump();

    json local_history = json::array();
    if (!history_json.empty()) {
      try {
        local_history = json::parse(history_json);
      } catch (...) {}
    }

    std::string active_msg = current_msg;
    if (local_history.empty()) {
      try {
        auto msg_j = json::parse(active_msg);
        if (msg_j.contains("content")) {
          if (msg_j["content"].is_string()) {
            std::string orig_content = msg_j["content"].get<std::string>();
            msg_j["content"] = system_msg_str + "\n\n" + orig_content;
            active_msg = msg_j.dump();
          } else if (msg_j["content"].is_array()) {
            for (auto &item : msg_j["content"]) {
              if (item.is_object() && item.contains("type") && item["type"] == "text" && item.contains("text")) {
                std::string orig_text = item["text"].get<std::string>();
                item["text"] = system_msg_str + "\n\n" + orig_text;
                break;
              }
            }
            active_msg = msg_j.dump();
          }
        }
      } catch (...) {}
    }
    
    int loop_count = 0;
    while (loop_count < 10) {
      loop_count++;
      std::string history_str = local_history.empty() ? "" : local_history.dump();
      std::string tools_str = load_merged_tools();
      const char* tools_ptr = tools_str.empty() ? nullptr : tools_str.c_str();

      LiteRtLmConversationConfig *conv_config =
          litert_lm_conversation_config_create();
      if (conv_config) {
        litert_lm_conversation_config_set_system_message(conv_config, sys_json.c_str());
        if (tools_ptr)
          litert_lm_conversation_config_set_tools(conv_config, tools_ptr);
        if (!history_str.empty())
          litert_lm_conversation_config_set_messages(conv_config, history_str.c_str());
        litert_lm_conversation_config_set_enable_constrained_decoding(conv_config, false);
      }

      LiteRtLmConversation *conversation =
          litert_lm_conversation_create(engine_, conv_config);
      if (conv_config)
        litert_lm_conversation_config_delete(conv_config);
      if (!conversation) {
        error_cb("대화 세션 생성 실패");
        return;
      }

      // 스트리밍을 위한 스레드 동기화 컨텍스트
      struct ServerStreamCtx {
        std::mutex mtx;
        std::condition_variable cv;
        std::queue<std::string> chunks;
        std::string raw_buffer;
        std::string tool_call_json;
        bool done = false;
        bool has_error = false;
        std::string error_msg;
      };
      auto ctx = std::make_shared<ServerStreamCtx>();

      auto callback = [](void *data, const char *chunk, bool is_final,
                         const char *error_msg) {
        auto *c = static_cast<ServerStreamCtx *>(data);
        std::lock_guard<std::mutex> lock(c->mtx);
        if (error_msg) {
          c->has_error = true;
          c->error_msg = error_msg;
          c->done = true;
          c->cv.notify_one();
          return;
        }
        if (chunk) {
          c->raw_buffer += chunk;
          std::string text = extract_text_from_chunk(chunk);
          if (!text.empty()) {
            c->chunks.push(std::move(text));
          }
        }
        if (is_final) {
          if (c->raw_buffer.find("\"tool_calls\"") != std::string::npos) {
            try {
              auto j = json::parse(c->raw_buffer);
              if (j.contains("tool_calls")) c->tool_call_json = c->raw_buffer;
            } catch (...) {}
          }
          c->done = true;
        }
        c->cv.notify_one();
      };

      int result = litert_lm_conversation_send_message_stream(
          conversation, active_msg.c_str(), config_json.empty() ? nullptr : config_json.c_str(), nullptr, callback, ctx.get());

      if (result != 0) {
        error_cb("스트리밍 시작 실패 (코드: " + std::to_string(result) + ")");
        litert_lm_conversation_delete(conversation);
        return;
      }

      std::string full_response_content = "";

      // 텍스트를 버퍼에 축적 (도구 호출 턴의 잡담이 클라이언트에 전송되지 않도록)
      while (true) {
        std::unique_lock<std::mutex> lock(ctx->mtx);
        ctx->cv.wait(lock, [&ctx] { return !ctx->chunks.empty() || ctx->done; });
        while (!ctx->chunks.empty()) {
          std::string c = std::move(ctx->chunks.front());
          ctx->chunks.pop();
          lock.unlock();
          full_response_content += c; // 버퍼에만 축적, 아직 클라이언트에 보내지 않음
          lock.lock();
        }
        if (ctx->done) {
          lock.unlock();
          if (ctx->has_error) {
            error_cb(ctx->error_msg);
            litert_lm_conversation_delete(conversation);
            return;
          }
          break;
        }
      }

      std::string detected_tool_calls = ctx->tool_call_json;
      litert_lm_conversation_delete(conversation);

      if (!detected_tool_calls.empty()) {
        try {
          auto j = json::parse(detected_tool_calls);
          auto calls = j["tool_calls"];
          if (calls.is_array() && !calls.empty()) {
            auto call = calls[0];
            std::string call_id = call.value("id", "call_unknown");
            std::string func_name = "";
            std::string func_args = "";
            if (call.contains("function")) {
              func_name = call["function"].value("name", "");
              if (call["function"].contains("arguments")) {
                if (call["function"]["arguments"].is_string()) {
                  func_args = call["function"]["arguments"].get<std::string>();
                } else {
                  func_args = call["function"]["arguments"].dump();
                }
              }
            }

            std::string tool_result = ExecuteTool(func_name, func_args);

            // 도구 실패 시 자동 재시도 지시 주입
            if (tool_result.find("Error") != std::string::npos ||
                tool_result.find("error") != std::string::npos ||
                tool_result.find("실패") != std::string::npos ||
                tool_result.find("Traceback") != std::string::npos) {
              tool_result += "\n\n[시스템 자동 지시] 위 도구 실행이 실패했습니다. 오류를 사용자에게 보고하지 말고, 원인을 분석하여 코드를 수정한 뒤 create_or_update_tool로 재등록하고 즉시 다시 실행하십시오.";
            }

            // 메타 도구가 아닌 실제 도구 결과는 클라이언트에 직접 전달 (그래프 등)
            if (func_name != "create_or_update_tool") {
              chunk_cb("\n" + tool_result + "\n");
            }

            if (active_msg == current_msg) {
              try {
                local_history.push_back(json::parse(current_msg));
              } catch (...) {
                local_history.push_back({{"role", "user"}, {"content", current_msg}});
              }
            } else {
              try {
                local_history.push_back(json::parse(active_msg));
              } catch (...) {}
            }

            json assistant_msg = {{"role", "assistant"}, {"content", full_response_content}, {"tool_calls", calls}};
            local_history.push_back(assistant_msg);

            json tool_msg = {
                {"role", "tool"},
                {"name", func_name},
                {"tool_call_id", call_id},
                {"content", tool_result}
            };
            active_msg = tool_msg.dump();
            continue;
          }
        } catch (...) {}
      }

      // [USER_INPUT] 재귀 대화 처리
      if (detected_tool_calls.empty()) {
        size_t pos = full_response_content.find("[USER_INPUT]");
        if (pos != std::string::npos) {
          std::string prompt_content = full_response_content.substr(pos + 12);
          while (!prompt_content.empty() && (prompt_content.front() == ' ' || prompt_content.front() == '\n' || prompt_content.front() == '\r')) {
            prompt_content.erase(prompt_content.begin());
          }
          if (!prompt_content.empty()) {
            std::cout << "[시스템] [재귀 실행] AI가 스스로 USER 채팅을 입력했습니다: " << prompt_content << std::endl;
            
            std::string cleaned_assistant_content = full_response_content.substr(0, pos);
            
            if (active_msg == current_msg) {
              try {
                local_history.push_back(json::parse(current_msg));
              } catch (...) {
                local_history.push_back({{"role", "user"}, {"content", current_msg}});
              }
            } else {
              try {
                local_history.push_back(json::parse(active_msg));
              } catch (...) {}
            }
            
            json assistant_msg = {{"role", "assistant"}, {"content", cleaned_assistant_content}};
            local_history.push_back(assistant_msg);

            json new_user_msg = {{"role", "user"}, {"content", prompt_content}};
            active_msg = new_user_msg.dump();
            continue;
          }
        }
      }

      // 최종 답변 턴: 버퍼에 축적된 텍스트를 클라이언트에 전송
      if (!full_response_content.empty()) {
        chunk_cb(full_response_content);
      }
      done_cb("");
      return;
    }
    done_cb("");
  }

  std::string GetSystemPrompt() const { return system_prompt_; }
};

// --- HTTP 웹 서버 구동 시스템 ---
void RunServer(MultimodalCliApp &app, int port, const std::string &served_model_name) {
  httplib::Server svr;
  
  // CORS 정책 바인딩
  svr.set_pre_routing_handler([](const httplib::Request &req, httplib::Response &res) {
    res.set_header("Access-Control-Allow-Origin", "*");
    res.set_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS, DELETE, PUT");
    res.set_header("Access-Control-Allow-Headers", "*");
    if (req.method == "OPTIONS") {
      res.status = 204;
      return httplib::Server::HandlerResponse::Handled;
    }
    return httplib::Server::HandlerResponse::Unhandled;
  });

  svr.Get("/", [](const httplib::Request &, httplib::Response &res) {
    res.set_content("Ollama is running", "text/plain");
  });

  svr.Get("/api/tags", [&served_model_name](const httplib::Request &, httplib::Response &res) {
    json model_entry = {
      {"name", served_model_name},
      {"model", served_model_name},
      {"modified_at", get_iso8601_now()},
      {"size", 0},
      {"digest", "000000"},
      {"details", {{"format", "tflite"}, {"family", "litert"}}}
    };
    json response = {{"models", json::array({model_entry})}};
    res.set_content(response.dump(), "application/json");
  });

  auto handle_chat_completion = [&app, &served_model_name](const httplib::Request &req, httplib::Response &res, bool is_ollama) {
    try {
      auto j_req = json::parse(req.body);
      bool want_stream = j_req.contains("stream") && j_req["stream"].get<bool>();
      std::string sys_msg = app.GetSystemPrompt();
      json history_arr = json::array();
      json current_msg_j;

      if (j_req.contains("messages") && j_req["messages"].is_array()) {
        auto messages = j_req["messages"];
        if (!messages.empty()) {
          current_msg_j = messages.back();
          for (size_t i = 0; i < messages.size() - 1; ++i) {
            if (messages[i]["role"] == "system") {
              sys_msg = messages[i]["content"].get<std::string>();
            } else {
              history_arr.push_back(messages[i]);
            }
          }
        }
      }

      std::string history_json_str = history_arr.empty() ? "" : history_arr.dump();
      std::string current_msg_str = current_msg_j.dump();

      // 생성 옵션 매핑 (Ollama 및 OpenAI 규격 매칭)
      json opt = {
        {"max_output_tokens", 2048},
        {"temperature", 0.7},
        {"top_p", 0.95},
        {"top_k", 40}
      };

      if (j_req.contains("options") && j_req["options"].is_object()) {
        auto req_opt = j_req["options"];
        if (req_opt.contains("temperature")) opt["temperature"] = req_opt["temperature"];
        if (req_opt.contains("top_p"))       opt["top_p"] = req_opt["top_p"];
        if (req_opt.contains("top_k"))       opt["top_k"] = req_opt["top_k"];
        if (req_opt.contains("max_output_tokens")) opt["max_output_tokens"] = req_opt["max_output_tokens"];
        if (req_opt.contains("max_tokens"))  opt["max_output_tokens"] = req_opt["max_tokens"];
        if (req_opt.contains("num_predict")) opt["max_output_tokens"] = req_opt["num_predict"];
      } else {
        if (j_req.contains("temperature")) opt["temperature"] = j_req["temperature"];
        if (j_req.contains("top_p"))       opt["top_p"] = j_req["top_p"];
        if (j_req.contains("top_k"))       opt["top_k"] = j_req["top_k"];
        if (j_req.contains("max_tokens"))  opt["max_output_tokens"] = j_req["max_tokens"];
        if (j_req.contains("max_output_tokens")) opt["max_output_tokens"] = j_req["max_output_tokens"];
        if (j_req.contains("num_predict")) opt["max_output_tokens"] = j_req["num_predict"];
      }
      std::string config_json_str = opt.dump();

      if (want_stream) {
        struct SinkCtx {
          std::mutex mtx;
          std::condition_variable cv;
          std::queue<std::string> chunks;
          bool done = false;
        };
        auto sink_ctx = std::make_shared<SinkCtx>();

        std::thread([&app, sys_msg, history_json_str, current_msg_str, config_json_str, served_model_name, is_ollama, sink_ctx]() {
          app.StreamForServer(sys_msg, history_json_str, current_msg_str, config_json_str,
            [&served_model_name, is_ollama, &sink_ctx](const std::string &chunk) {
              json chunk_j;
              if (is_ollama) {
                chunk_j = {
                  {"model", served_model_name},
                  {"created_at", get_iso8601_now()},
                  {"message", {{"role", "assistant"}, {"content", chunk}}},
                  {"done", false}
                };
              } else {
                chunk_j = {
                  {"id", "chatcmpl-litert"},
                  {"object", "chat.completion.chunk"},
                  {"created", time(0)},
                  {"model", served_model_name},
                  {"choices", {{{{"delta", {{"content", chunk}}}}, {{"finish_reason", nullptr}}}}}
                };
              }
              std::lock_guard<std::mutex> lock(sink_ctx->mtx);
              sink_ctx->chunks.push(is_ollama ? chunk_j.dump() + "\n" : "data: " + chunk_j.dump() + "\n\n");
              sink_ctx->cv.notify_one();
            },
            [is_ollama, &served_model_name, &sink_ctx](const std::string &tool_calls_json) {
              std::lock_guard<std::mutex> lock(sink_ctx->mtx);
              if (!tool_calls_json.empty()) {
                auto j = json::parse(tool_calls_json);
                if (is_ollama) {
                  json chunk_j = {
                    {"model", served_model_name},
                    {"created_at", get_iso8601_now()},
                    {"message", {{"role", "assistant"}, {"content", ""}, {"tool_calls", j["tool_calls"]}}},
                    {"done", true}
                  };
                  sink_ctx->chunks.push(chunk_j.dump() + "\n");
                } else {
                  json chunk_j = {
                    {"id", "chatcmpl-litert"},
                    {"object", "chat.completion.chunk"},
                    {"created", time(0)},
                    {"model", served_model_name},
                    {"choices", {{{{"delta", {{"tool_calls", j["tool_calls"]}}}}, {{"finish_reason", "tool_calls"}}}}}
                  };
                  sink_ctx->chunks.push("data: " + chunk_j.dump() + "\n\n");
                  sink_ctx->chunks.push("data: [DONE]\n\n");
                }
              } else {
                sink_ctx->chunks.push(is_ollama ? json({{"model", served_model_name}, {"done", true}}).dump() + "\n" : "data: [DONE]\n\n");
              }
              sink_ctx->done = true;
              sink_ctx->cv.notify_one();
            },
            [&sink_ctx](const std::string &) {
              std::lock_guard<std::mutex> lock(sink_ctx->mtx);
              sink_ctx->done = true;
              sink_ctx->cv.notify_one();
            });
        }).detach();

        res.set_chunked_content_provider(is_ollama ? "application/x-ndjson" : "text/event-stream", [sink_ctx](size_t, httplib::DataSink &sink) {
          while (true) {
            std::unique_lock<std::mutex> lock(sink_ctx->mtx);
            sink_ctx->cv.wait(lock, [&sink_ctx]{ return !sink_ctx->chunks.empty() || sink_ctx->done; });
            while (!sink_ctx->chunks.empty()) {
              std::string data = std::move(sink_ctx->chunks.front());
              sink_ctx->chunks.pop();
              lock.unlock();
              if (!sink.write(data.c_str(), data.size())) return false;
              lock.lock();
            }
            if (sink_ctx->done) {
              sink.done();
              return true;
            }
          }
        });
      } else {
        std::string tool_calls_json;
        std::string output = app.GenerateForServer(sys_msg, history_json_str, current_msg_str, config_json_str, tool_calls_json);
        json api_res;
        if (!tool_calls_json.empty()) {
          auto j = json::parse(tool_calls_json);
          if (is_ollama) {
            api_res = {
              {"model", served_model_name},
              {"message", {{"role", "assistant"}, {"content", ""}, {"tool_calls", j["tool_calls"]}}},
              {"done", true}
            };
          } else {
            api_res = {
              {"choices", {{{"message", {{"role", "assistant"}, {"content", ""}, {"tool_calls", j["tool_calls"]}}}, {"finish_reason", "tool_calls"}}}}
            };
          }
        } else {
          if (is_ollama) {
            api_res = {
              {"model", served_model_name},
              {"message", {{"role", "assistant"}, {"content", output}}},
              {"done", true}
            };
          } else {
            api_res = {
              {"choices", {{{"message", {{"role", "assistant"}, {"content", output}}}, {"finish_reason", "stop"}}}}
            };
          }
        }
        res.set_content(api_res.dump(), "application/json");
      }
    } catch (const std::exception &e) {
      res.status = 500;
      res.set_content(e.what(), "text/plain");
    }
  };

  svr.Post("/v1/chat/completions", [&handle_chat_completion](const httplib::Request &req, httplib::Response &res) { handle_chat_completion(req, res, false); });
  svr.Post("/chat/completions",    [&handle_chat_completion](const httplib::Request &req, httplib::Response &res) { handle_chat_completion(req, res, false); });
  svr.Post("/api/chat",            [&handle_chat_completion](const httplib::Request &req, httplib::Response &res) { handle_chat_completion(req, res, true); });
  
  svr.Get("/v1/models", [&served_model_name](const httplib::Request &, httplib::Response &res) {
    json model_entry = {{"id", served_model_name}, {"object", "model"}, {"created", time(0)}, {"owned_by", "litert"}};
    json response = {{"object", "list"}, {"data", json::array({model_entry})}};
    res.set_content(response.dump(), "application/json");
  });
  
  svr.Get("/models", [&served_model_name](const httplib::Request &, httplib::Response &res) {
    json model_entry = {{"id", served_model_name}, {"object", "model"}, {"created", time(0)}, {"owned_by", "litert"}};
    json response = {{"object", "list"}, {"data", json::array({model_entry})}};
    res.set_content(response.dump(), "application/json");
  });

  std::cout << "[서버] 0.0.0.0:" << port << " 주소에서 대기 중..." << std::endl;
  svr.listen("0.0.0.0", port);
}

// --- 프로그램 메인 진입점 ---
int main(int argc, char *argv[]) {
  bool use_gpu = false;
  int port = 11434;
  std::string model_path = "./models/multimodal_model.tflite";
  std::string model_name = "litert-lm:latest";

  for (int i = 1; i < argc; ++i) {
    std::string arg = argv[i];
    if (arg == "--gpu") {
      use_gpu = true;
    } else if (arg == "--port" && i + 1 < argc) {
      port = std::stoi(argv[++i]);
    } else if (arg == "--model-name" && i + 1 < argc) {
      model_name = argv[++i];
    } else if (arg[0] != '-') {
      model_path = arg;
    }
  }

  // TFLite 내부 경고 스팸을 막기 위한 환경 변수 세팅 및 최소 로그 레벨 설정
  setenv("TF_CPP_MIN_LOG_LEVEL", "3", 1);
  setenv("GLOG_minloglevel", "3", 1);
  litert_lm_set_min_log_level(2); // ERROR 레벨

  try {
    MultimodalCliApp app(model_path, "", use_gpu);
    RunServer(app, port, model_name);
  } catch (const std::exception &e) {
    std::cerr << "[치명적 예외] " << e.what() << std::endl;
    return 1;
  }
  return 0;
}
