// ratatui 화면 레이아웃 조율을 위해 제약조건(Constraint), 화면 분기 방향(Direction), 레이아웃 컴포넌트(Layout)를 임포트합니다.
use ratatui::{
    layout::{Constraint, Direction, Layout},
    // 스타일 배정을 위한 색상(Color), 글꼴 수식자(Modifier), 스타일 양식(Style)을 가져옵니다.
    style::{Color, Modifier, Style},
    // 텍스트 단편들을 조립해 한 줄 문장을 형성하기 위한 Line 및 Span 구조체를 임포트합니다.
    text::{Line, Span},
    // 블록 테두리(Block), 테두리 경계선 지시자(Borders), 단락 위젯(Paragraph), 줄바꿈 규칙(Wrap)을 임포트합니다.
    widgets::{Block, Borders, Paragraph, Wrap},
    // 프레임 캔버스 렌더링 영역 참조를 위해 Frame을 가져옵니다.
    Frame,
};

// TuiApp 상태값 및 분기 판 식별을 위해 모듈 구조체들을 임포트합니다.
use crate::app::{TuiApp, ActivePane};

// TuiApp 구조체에 대한 TUI 그래픽 화면 렌더링 그리기(draw) 함수를 구현해 올립니다.
impl TuiApp {
    // 터미널 캔버스 캔버스 공간(Frame)을 전달받아 레이아웃을 나누고 위젯 데이터를 채워 그려주는 함수입니다.
    pub fn draw(&self, f: &mut Frame) {
        // 화면 하단에 타자 입력창(Constraint::Length(3))을 배정하고 상단 전체를 주 작업 영역으로 수직 2등분 분할합니다.
        let main_chunks = Layout::default()
            // 상하 세로(Vertical) 방향으로 배치
            .direction(Direction::Vertical)
            // 상단 자동 최대 크기(Min(3)), 하단 고정 높이 3 할당
            .constraints([
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            // 지정 캔버스 전체 해상도 크기를 할당받아 쪼갭니다.
            .split(f.size());

        // 상단 주 영역을 좌측 대화 역사 영역(60%), 우측 에이전트 정보판(40%)의 비율로 가로 2등분 분할합니다.
        let content_chunks = Layout::default()
            // 좌우 가로(Horizontal) 방향 분기
            .direction(Direction::Horizontal)
            // 비율 설정
            .constraints([
                Constraint::Percentage(60),
                Constraint::Percentage(40),
            ])
            // 상단 메인 청크 영역을 인계받아 나눕니다.
            .split(main_chunks[0]);

        // 좌측의 대화 내역 전체를 담는 상단 60% 영역과 에이전트 자율 도구 실행 타임라인을 나타내는 하단 40% 영역으로 세로 2등분 분할합니다.
        let left_chunks = Layout::default()
            // 상하 수직 배치
            .direction(Direction::Vertical)
            // 6:4 세로 배치 설정
            .constraints([
                Constraint::Percentage(60),
                Constraint::Percentage(40),
            ])
            // 좌측 주 영역 청크를 전달받아 쪼갭니다.
            .split(content_chunks[0]);

        // --- 좌측 상단: 대화 기록 렌더링 전처리 ---
        // 렌더링에 적재할 줄(Line) 단위 개별 요소들의 목록 벡터를 구성합니다.
        let mut chat_lines = Vec::new();
        // 메모리에 누적 적재된 (role, content) 대화 히스토리 순회를 개시합니다.
        for (role, content) in &self.chat_history {
            // 발화자가 유저(user)인 경우의 UI 스타일 데코레이션입니다.
            if role == "user" {
                // 발문 서두 심볼(User ❯)을 하늘색 볼드체 스타일로 스팬을 빌드하고 유저 발화문 텍스트를 배치합니다.
                chat_lines.push(Line::from(vec![
                    Span::styled("User ❯ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(content, Style::default().fg(Color::White)),
                ]));
                // 가독성 확보를 위해 줄 간에 가볍게 빈 여백 줄을 하나 삽입해 줍니다.
                chat_lines.push(Line::from(""));
            } else {
                // 발화자가 AI 어시스턴트인 경우의 UI 데코레이션 가공입니다.
                // 발문 서두 기호(AI ❯)를 자홍색 볼드체로 형성하고 연한 녹색 텍스트로 AI 생성 대답 본안을 배치합니다.
                chat_lines.push(Line::from(vec![
                    Span::styled("AI ❯ ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                    Span::styled(content, Style::default().fg(Color::LightGreen)),
                ]));
                // 줄 간 빈 줄 단편 추가
                chat_lines.push(Line::from(""));
            }
        }

        // 현재 백그라운드 추론 스트리밍이 돌면서 실시간으로 문장을 빚어내는 중일 때의 임시 실시간 출력부입니다.
        if !self.current_assistant_response.is_empty() {
            // 연한 녹색 텍스트로 마저 조합 중인 본문을 실시간 업데이트하여 가시화합니다.
            chat_lines.push(Line::from(vec![
                Span::styled("AI ❯ ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(&self.current_assistant_response, Style::default().fg(Color::LightGreen)),
            ]));
        }

        // 대화 히스토리의 전체 줄 높이 수량을 확보합니다.
        let total_lines = chat_lines.len() as u16;
        // 테두리 두께(위아래 고정 마진 2라인)를 제하고 실제 내부 Paragraph 위젯이 표현되는 한계 높이를 획득합니다.
        let chat_height = left_chunks[0].height.saturating_sub(2);
        // 스크롤 포커스가 미설정(0)인 기본 작동 상태에서는 대화 스택 최하단(최신 줄)이 자동으로 노출되도록 강제 계산 조율합니다.
        let current_scroll = if self.chat_scroll == 0 {
            // 전체 줄 높이에서 보여지는 단락 영역 높이를 감산한 나머지 값으로 자동 픽스합니다.
            total_lines.saturating_sub(chat_height)
        } else {
            // 사용자가 방향키로 직접 스크롤링을 변경한 고유 인덱스 값을 그대로 존중합니다.
            self.chat_scroll
        };

        // 키보드 입력 포커스(active_pane)가 Chat 창에 매핑되어 있을 때만 테두리 선 색상을 강조용 하늘색(Cyan)으로 표기합니다.
        let chat_border = if self.active_pane == ActivePane::Chat { Color::Cyan } else { Color::White };
        // 단락 단문(Paragraph) 위젯에 준비된 라인 텍스트와 테두리 스타일 정보들을 가미하여 가공 빌드합니다.
        let chat_para = Paragraph::new(chat_lines)
            // 위젯 외곽에 borders 두께 및 제목(💬 Chat History) 세팅
            .block(Block::default().borders(Borders::ALL).title(" 💬 Chat History ").border_style(Style::default().fg(chat_border)))
            // 경계를 초과하는 긴 줄은 자동으로 절단 개행시키는 wrap 정렬 구성
            .wrap(Wrap { trim: true })
            // 세로축 계산된 오프셋 기준으로 스크롤 적용
            .scroll((current_scroll, 0));
        // 가공 빌드된 단락 위젯을 좌측 상단 청크 영역에 투영 렌더링합니다.
        f.render_widget(chat_para, left_chunks[0]);

        // --- 좌측 하단: 에이전트 자율 도구 실행 타임라인(Recursive Tool Calls) 렌더링 ---
        // 렌더링에 사용할 줄 단위 목록 벡터를 선언합니다.
        let mut recursive_lines = Vec::new();
        // 타임라인 내역 데이터 벡터를 순회 스캔합니다.
        for log in &self.recursive_logs {
            // 출력 로그 속성에 부합하는 전용 표시 색상을 판단 조율합니다.
            let color = if log.contains("[도구 호출]") {
                // 노란색 배정
                Color::Yellow
            } else if log.contains("[도구 결과]") {
                // 초록색 지정
                Color::Green
            } else if log.contains("[시스템 안내]") {
                // 하늘색 매핑
                Color::Cyan
            } else if log.contains("오류") {
                // 오류 메시지는 빨간색 강조
                Color::Red
            } else {
                // 기본 평문은 백색 처리
                Color::White
            };
            // 스타일을 입힌 단일 스팬 줄 객체로 만들어 벡터에 탑재시킵니다.
            recursive_lines.push(Line::from(Span::styled(log, Style::default().fg(color))));
        }

        // 테두리를 제외한 가용 단락 높이를 확보합니다.
        let rec_height = left_chunks[1].height.saturating_sub(2);
        // 마지막 최신 로그 줄 위치로의 자동 스크롤 값을 조율 계산해 둡니다.
        let auto_rec_scroll = (recursive_lines.len() as u16).saturating_sub(rec_height);
        // 현재 포커스 창이 Recursive이면서 방향키 상하 입력값이 유효하다면 그 수치를 따르고, 그 외에는 자동 하단 고정 스크롤을 적용합니다.
        let rec_scroll = if self.active_pane == ActivePane::Recursive && self.recursive_scroll > 0 { self.recursive_scroll } else { auto_rec_scroll };

        // 창 활성화 상태 여부를 판별하여 외곽 테두리 하이라이팅 색상을 배정합니다.
        let rec_border = if self.active_pane == ActivePane::Recursive { Color::Cyan } else { Color::White };
        // 단락 위젯 구성에 들어갑니다.
        let rec_para = Paragraph::new(recursive_lines)
            // 타이틀 타이틀 명명 및 테두리 인가
            .block(Block::default().borders(Borders::ALL).title(" 🔁 Recursive Tool Calls (Agent Reasoning) ").border_style(Style::default().fg(rec_border)))
            // 자동 자르기 적용
            .wrap(Wrap { trim: true })
            // 스크롤 포커스 적용
            .scroll((rec_scroll, 0));
        // 가공 완수된 위젯을 좌측 하단 할당 청크 영역에 렌더링합니다.
        f.render_widget(rec_para, left_chunks[1]);

        // --- 우측 영역: 정보 판 레이아웃 수직 2분할 (상단: 진행판 60%, 하단: 콘솔 로그 40%) ---
        let right_chunks = Layout::default()
            // 상하 수직 배치
            .direction(Direction::Vertical)
            // 6:4 비율 설정
            .constraints([
                Constraint::Percentage(60),
                Constraint::Percentage(40),
            ])
            // 우측 메인 청크 영역 인계 후 분기
            .split(content_chunks[1]);

        // --- 우측 상단: 작업 진행 및 상세 계획 보드(Agentic Board) 렌더링 ---
        // 진행판 전용 출력 텍스트 목록 벡터를 준비합니다.
        let mut board_lines = Vec::new();
        // 헤더 뱃지 타이틀을 볼드 노란색으로 주입합니다.
        board_lines.push(Line::from(Span::styled("📊 작업 진행 상태", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
        
        // 에이전트 자율 공정 항목 리스트가 비어 있는 경우
        if self.agent_status.is_empty() {
            // 대기 중 멘트를 짙은 회색으로 약하게 인쇄합니다.
            board_lines.push(Line::from(Span::styled("  대기 중...", Style::default().fg(Color::DarkGray))));
        } else {
            // 들어 있는 개별 상태 지문 라인들을 추출 순회합니다.
            for line in &self.agent_status {
                // 콜론 기호(:) 기준 앞쪽의 작업명과 뒷쪽의 상태 값을 명확히 반 잘라 추출합니다.
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                // 원형 정합성 규약 통과 시 분기 처리합니다.
                if parts.len() == 2 {
                    // 키 항목 공백 정리
                    let key = parts[0].trim();
                    // 밸류 항목 공백 정합
                    let val = parts[1].trim();
                    // 공정 완료 여부에 맞추어 미학적인 이모지 심볼과 색상을 유동 매핑시킵니다.
                    let (icon, color) = match val {
                        // 완료 공정 처리
                        "완료" => ("✅ 완료", Color::Green),
                        // 진행 중 처리
                        "진행" => ("⏳ 진행", Color::Yellow),
                        // 대기 공정 처리
                        "대기" => ("💤 대기", Color::DarkGray),
                        // 실패 공정 처리
                        "실패" => ("❌ 실패", Color::Red),
                        // 기타 변칙 문구 대응
                        _ => (val, Color::White),
                    };
                    // 줄 데이터 구조로 조립 결속하여 진행판에 탑재합니다.
                    board_lines.push(Line::from(vec![
                        // 키 타이틀 명시
                        Span::styled(format!("  • {:<12} : ", key), Style::default().fg(Color::White)),
                        // 가시 뱃지 및 색상 주입
                        Span::styled(icon, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    ]));
                } else {
                    // 자름 규약이 빗나가는 평문 지문은 원본 그대로 쏟아 인쇄합니다.
                    board_lines.push(Line::from(line.as_str()));
                }
            }
        }

        // 보드의 파트 구분을 짓기 위해 개행 빈 칸 줄을 연출 삽입합니다.
        board_lines.push(Line::from(""));
        // 상세 계획서 서두 항목 타이틀을 인쇄합니다.
        board_lines.push(Line::from(Span::styled("📋 상세 작업 계획", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
        
        // 세부 계획 수립 리스트가 부재할 시 대기 처리합니다.
        if self.agent_plan.is_empty() {
            // 계획 없음 지문 인쇄
            board_lines.push(Line::from(Span::styled("  계획 없음", Style::default().fg(Color::DarkGray))));
        } else {
            // 수립 계획 리스트 항목 순회
            for line in &self.agent_plan {
                // 상태 지시문 텍스트 토큰이 함유되어 있는지 탐색합니다.
                if line.contains("상태:") {
                    // "상태:" 지시어 기준으로 앞뒤를 슬라이싱 분리합니다.
                    let parts: Vec<&str> = line.split("상태:").collect();
                    // 단계 명칭 획득
                    let step = parts[0].trim();
                    // 진행 여부 상태 획득
                    let state = parts[1].trim();
                    // 기동 완료 상태 매핑 이모지를 배정 조율합니다.
                    let (icon, color) = match state {
                        "완료" => ("✅ 완료", Color::Green),
                        "진행" => ("⏳ 진행", Color::Yellow),
                        "대기" => ("💤 대기", Color::DarkGray),
                        "실패" => ("❌ 실패", Color::Red),
                        _ => (state, Color::White),
                    };
                    // 정렬 문장으로 합본해 계획보드 라인에 편입 가산합니다.
                    board_lines.push(Line::from(vec![
                        // 진행 단계 화살표
                        Span::styled(format!("  {} ➔ ", step), Style::default().fg(Color::White)),
                        // 이모지 뱃지 매핑
                        Span::styled(icon, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    ]));
                } else {
                    // 이외 구조 지문은 있는 글 그대로 라인 벡터에 추가합니다.
                    board_lines.push(Line::from(line.as_str()));
                }
            }
        }

        // 테두리를 제한 보드 가용 높이 산출
        let board_height = right_chunks[0].height.saturating_sub(2);
        // 하단 끝자락 고정 유도를 위한 기본 스크롤 값 계산
        let auto_board_scroll = (board_lines.len() as u16).saturating_sub(board_height);
        // 포커스 상태 및 조작 상태 판독하여 오프셋 결정
        let board_scroll_val = if self.active_pane == ActivePane::Board && self.board_scroll > 0 { self.board_scroll } else { auto_board_scroll };

        // 창 테두리 하이라이팅 색상 부여
        let board_border = if self.active_pane == ActivePane::Board { Color::Cyan } else { Color::White };
        // 보드 단락 위젯 완성
        let board_para = Paragraph::new(board_lines)
            .block(Block::default().borders(Borders::ALL).title(" 📊 Agentic Board ").border_style(Style::default().fg(board_border)))
            .wrap(Wrap { trim: true })
            .scroll((board_scroll_val, 0));
        // 우측 상단 렌더 영역에 배치
        f.render_widget(board_para, right_chunks[0]);

        // --- 우측 하단: 도구 컴파일 및 로우 실행 결과 로그(Execution Logs) 렌더링 ---
        // 로그 라인 수집 벡터 구성
        let mut log_lines = Vec::new();
        // 도구 로그 벡터 스캔
        for log in &self.tool_logs {
            // 해당 공정에 따른 시각 하이라이트 칼라 매핑을 구성합니다.
            let color = if log.contains("도구를 만듭니다") {
                // 생성 로그는 초록색
                Color::Green
            } else if log.contains("도구를 사용합니다") {
                // 사용 로그는 노란색
                Color::Yellow
            } else if log.contains("[시스템 자동 지시]") || log.contains("오류") {
                // 시스템 긴급 지령 및 실행 예외 스택은 빨간색 강조
                Color::Red
            } else {
                // 기본 글귀 흰색
                Color::White
            };
            // 개별 라인 탑재
            log_lines.push(Line::from(Span::styled(log, Style::default().fg(color))));
        }

        // 마진을 제외한 내부 가용 라인수 산출
        let logs_height = right_chunks[1].height.saturating_sub(2);
        // 최하단 자동 포커싱을 위한 기본 오프셋 연산
        let auto_logs_scroll = (log_lines.len() as u16).saturating_sub(logs_height);
        // 조작 정보 수렴해 스크롤 적용
        let logs_scroll_val = if self.active_pane == ActivePane::Logs && self.logs_scroll > 0 { self.logs_scroll } else { auto_logs_scroll };

        // 테두리 빔 컬러 선별
        let logs_border = if self.active_pane == ActivePane::Logs { Color::Cyan } else { Color::White };
        // 단락 위젯 조립
        let log_para = Paragraph::new(log_lines)
            .block(Block::default().borders(Borders::ALL).title(" 🛠️ Execution Logs ").border_style(Style::default().fg(logs_border)))
            .wrap(Wrap { trim: true })
            .scroll((logs_scroll_val, 0));
        // 우측 하단 지정 좌표계에 렌더링
        f.render_widget(log_para, right_chunks[1]);

        // --- 최하단: 사용자 키보드 입력 및 실시간 기동 상태창 (Input Area) ---
        // 온도 계수 및 직전 턴의 토큰 분석/생성 비용 명세를 반영한 인쇄 타이틀을 동적 생성합니다.
        let mut input_title = format!(
            " ⌨️ Input (Temp: {}, Max Tokens: 260k | Last: P:{}/C:{}/T:{}) ",
            self.settings.temperature,
            self.last_prompt_tokens,
            self.last_completion_tokens,
            self.last_prompt_tokens + self.last_completion_tokens
        );
        // 사용자가 대기 시킨 이미지 첨부 경로가 존재할 경우 타이틀 앞단에 강조 표시합니다.
        if let Some(ref img) = self.pending_image {
            // 이미지 첨부 마크 추가
            input_title = format!(" 🖼 [Attached: {}] {}", img, input_title.trim());
        }
        
        // 현재 AI 자율 루프 백그라운드 연산 작동 시 테두리 색을 노란색(Yellow) 경고로 돌리고 대기 모드 시 파란색(Blue)으로 안착시킵니다.
        let border_color = if self.is_loading { Color::Yellow } else { Color::Blue };
        // 연산 실행 중에는 유저 입력 작성을 회색으로 가로막고 대기 멘트를 띄우며, 일반 대기 시 유저 기입 텍스트를 백색 출력합니다.
        let input_style = if self.is_loading { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::White) };

        // 표시할 텍스트 선별
        let display_input = if self.is_loading {
            // 로딩 안내 멘트
            "AI 생각 중/도구 자율 동작 중... 잠시 기다려주세요."
        } else {
            // 유저가 입력한 글귀 원형
            &self.input
        };

        // 입력 영역 단락 위젯 마감 빌드
        let input_para = Paragraph::new(display_input)
            .style(input_style)
            .block(Block::default().borders(Borders::ALL).title(input_title).border_style(Style::default().fg(border_color)));
        // 최하단 청크 영역에 렌더링을 격발합니다.
        f.render_widget(input_para, main_chunks[1]);
    }
}
