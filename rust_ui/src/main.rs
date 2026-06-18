// CLI 인자 파싱 처리를 위해 clap의 Parser 트레이트를 가져옵니다.
use clap::Parser;
// 표준 라이브러리 입력/출력을 위해 io 모듈을 가져옵니다.
use std::io;
// 20ms 논블로킹 키보드 폴링 시간 간격을 조율하기 위해 Duration 구조체를 임포트합니다.
use std::time::Duration;
// 비동기 비동기 네트워크 스레드 통신 중계를 위해 tokio 채널 모듈을 임포트합니다.
use tokio::sync::mpsc;

// ratatui 화면 출력을 구성할 크로스터 프레임워크 백엔드 구조체 및 터미널 캔버스 제어기를 임포트합니다.
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
// crossterm 터미널 환경 제어 매크로 및 마우스 캡처, 로우 모드 스위치 함수들을 임포트합니다.
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

// 분할 설계된 각 서브 기능 모듈들의 모듈 트리 선언부입니다.
// 하이퍼파라미터 및 프롬프트 환경 설정을 관장하는 모듈입니다.
mod settings;
// NDJSON 구문 분석 및 통신 상태 이벤트를 처리하는 모듈입니다.
mod events;
// TUI 앱 상태 머신 및 비동기 POST 챗 요청을 관리하는 모듈입니다.
mod app;
// Ratatui 화면 분할 렌더링 및 UI 스타일링을 수행하는 모듈입니다.
mod ui;

// 로컬 설정 데이터를 읽어오기 위해 settings 모듈에서 함수를 가져옵니다.
use settings::load_settings;
// 통신 이벤트 포장 해독을 위해 events 모듈에서 열거형을 수입합니다.
use events::NetworkEvent;
// UI 컨트롤러 구조체 및 스크롤 포커스 식별자를 app 모듈에서 임포트합니다.
use app::{TuiApp, ActivePane};

// CLI 구동 시 터미널 인수 파라미터를 규정하는 인수 해석용 구조체 정의입니다.
#[derive(Parser, Debug)]
#[command(author, version, about = "LiteRT-LM Multimodal Chat Client", long_about = None)]
struct Args {
    // LLM 추론 서버가 바인딩된 호스트 주소(Host IP)를 지정하는 인자입니다.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    // LLM 서버의 포트 번호를 가리키는 인자입니다.
    #[arg(long, default_value = "11434")]
    port: u16,
}

// 비동기 런타임 진입 매크로를 선포하고 TUI 애플리케이션의 메인 루프를 시동합니다.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 터미널 인자들을 파싱하여 변수에 매핑해 줍니다.
    let args = Args::parse();
    // 타깃 접속 서버 주소를 HTTP URL 형식으로 포매팅 조립합니다.
    let server_url = format!("http://{}:{}", args.host, args.port);

    // 디스크에서 기존 config.json 파라미터 설정을 로드합니다.
    let settings = load_settings();

    // 키보드 입력을 날것 그대로 가로채서 처리하기 위해 크로스터 로우 모드(raw mode)를 작동시킵니다.
    enable_raw_mode()?;
    // 표준 출력을 참조 확보합니다.
    let mut stdout = io::stdout();
    // 터미널 전체 화면(EnterAlternateScreen)을 교체하고 마우스 이벤트를 접수 캡처(EnableMouseCapture) 처리합니다.
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    // CrosstermBackend 렌더링 타깃 인스턴스를 빌드합니다.
    let backend = CrosstermBackend::new(stdout);
    // 라타투이 터미널 그리개 엔진을 생성 기동합니다.
    let mut terminal = Terminal::new(backend)?;

    // TuiApp 전체 상태 관리자를 메모리에 로드해 인스턴스화합니다.
    let mut app = TuiApp::new(settings, server_url);
    // 시작 알림 멘트를 UI 안내용 로그판에 순차 적재합니다.
    app.tool_logs.push("✨ LiteRT-LM Multimodal TUI Client가 시작되었습니다.".to_string());
    app.tool_logs.push("  • 명령어: /clear (기록 초기화), /img <경로> (이미지 첨부)".to_string());
    app.tool_logs.push("  • 파라미터 명령어: /temp <값>, /top_p <값>, /max_tokens <값>, /prompt <텍스트>".to_string());
    app.tool_logs.push("  • 종료: Esc 키 또는 /exit".to_string());

    // 비동기 네트워크 스레드로부터 도착할 통신 이벤트를 수령하기 위해 tokio 채널(용량 100)을 개설합니다.
    let (net_tx, mut net_rx) = mpsc::channel::<NetworkEvent>(100);

    // 이벤트 대기 및 터미널 렌더링 무한 루프를 기동합니다.
    loop {
        // 현재 앱의 상태값(TuiApp) 정보를 라타투이 캔버스에 전달하여 전면 리페인팅을 실시합니다.
        terminal.draw(|f| app.draw(f))?;

        // 20ms 주기로 단기 논블로킹 키보드 이벤트 폴링(poll) 검사를 격발합니다.
        if event::poll(Duration::from_millis(20))? {
            // 키보드 키 입력 이벤트를 스캔해 냅니다.
            if let Event::Key(key) = event::read()? {
                // 특정 키조합 패턴을 해석 분기합니다.
                match key.code {
                    // 사용자가 Ctrl + C 키를 타격 시 비상 탈출하여 종료 분기로 이행합니다.
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // 루프 이탈
                        break;
                    }
                    // 일반 일반 문자 키가 접수된 경우의 처리입니다.
                    KeyCode::Char(c) => {
                        // AI 생각 작동 중(is_loading)이 아닐 때만 입력창 버퍼에 문자를 가산 축적합니다.
                        if !app.is_loading {
                            // 문자 가산
                            app.input.push(c);
                        }
                    }
                    // 백스페이스 키 타격 시 글씨를 지워 줍니다.
                    KeyCode::Backspace => {
                        // 대기 모드 시에만 지우기 작동
                        if !app.is_loading {
                            // 지우기
                            app.input.pop();
                        }
                    }
                    // 엔터 키를 쳤을 때의 대화 발송 또는 셸 명령 해석 처리입니다.
                    KeyCode::Enter => {
                        // 로딩 대기가 아니며 타자한 입력 내용이 공백이 아닐 경우
                        if !app.is_loading && !app.input.trim().is_empty() {
                            // 입력 버퍼를 가로채어 임시 수거합니다.
                            let msg = app.input.trim().to_string();
                            // 입력판 비우기
                            app.input.clear();
                            // 사용자가 "/exit" 명령으로 마감을 유도 시 루프를 즉시 탈출합니다.
                            if msg == "/exit" {
                                break;
                            }
                            // 슬래시로 시작하는 제어 지시어인 경우
                            if msg.starts_with('/') {
                                // 셸 변경 명령 핸들러 구동
                                app.handle_command(&msg);
                            } else {
                                // 일반 질문인 경우 비동기 HTTP 채널을 실어 대화를 발송합니다.
                                app.send_message(msg, net_tx.clone());
                            }
                        }
                    }
                    // Esc 키를 누르면 클라이언트를 마감 종료합니다.
                    KeyCode::Esc => {
                        break;
                    }
                    // Tab 키를 누르면 방향키로 스크롤할 포커스 창(Pane)이 순환 전환됩니다.
                    KeyCode::Tab => {
                        // Chat -> Recursive -> Board -> Logs 순환
                        app.active_pane = match app.active_pane {
                            ActivePane::Chat => ActivePane::Recursive,
                            ActivePane::Recursive => ActivePane::Board,
                            ActivePane::Board => ActivePane::Logs,
                            ActivePane::Logs => ActivePane::Chat,
                        };
                    }
                    // 방향키 위(Up) 버튼 입력 시 스크롤 포커스가 할당된 대상 창의 스크롤 인덱스를 감산합니다.
                    KeyCode::Up => {
                        // 포커스 창 분기
                        match app.active_pane {
                            // 대화창 스크롤 조절
                            ActivePane::Chat => { if app.chat_scroll > 0 { app.chat_scroll -= 1; } }
                            // 에이전트 타임라인 조절
                            ActivePane::Recursive => { if app.recursive_scroll > 0 { app.recursive_scroll -= 1; } }
                            // 계획보드 조절
                            ActivePane::Board => { if app.board_scroll > 0 { app.board_scroll -= 1; } }
                            // 로그판 조절
                            ActivePane::Logs => { if app.logs_scroll > 0 { app.logs_scroll -= 1; } }
                        }
                    }
                    // 방향키 아래(Down) 버튼 입력 시 대상 활성창의 스크롤 인덱스를 증산시킵니다.
                    KeyCode::Down => {
                        // 스크롤 가산
                        match app.active_pane {
                            // 대화창 스크롤 다운
                            ActivePane::Chat => { app.chat_scroll += 1; }
                            // 타임라인 다운
                            ActivePane::Recursive => { app.recursive_scroll += 1; }
                            // 계획보드 다운
                            ActivePane::Board => { app.board_scroll += 1; }
                            // 로그판 다운
                            ActivePane::Logs => { app.logs_scroll += 1; }
                        }
                    }
                    // 이외 기타 조작 키 코드는 매칭을 통과 처리합니다.
                    _ => {}
                }
            }
        }

        // 비동기 소켓 채널 수령 대기를 논블로킹(try_recv) 방식으로 고속 수거하여 상태판에 연속 가산합니다.
        while let Ok(net_ev) = net_rx.try_recv() {
            // 입수된 이벤트 매핑 처리 구동
            app.handle_network_event(net_ev);
        }
    }

    // 터미널 환경을 기존의 날것으로 복구 회수 조치합니다.
    // 날것의 로우 모드 오프 처리
    disable_raw_mode()?;
    // 대체 스크린 복귀 및 마우스 캡처 회수
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    // 커서 복조 표식 처리
    terminal.show_cursor()?;

    // 무사 종결 리포트 반환
    Ok(())
}
