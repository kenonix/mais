#!/usr/bin/env python3
"""
plot_function — 수학 함수 그래프 렌더러 (2D/3D)
유니코드 점자 패턴 + ANSI 색상으로 터미널에 고해상도 그래프를 그립니다.
개발자가 직접 편집하여 기능을 확장할 수 있는 외부 도구입니다.
"""
import math
import sys
import json

# ─── ANSI 색상 코드 ───
RESET   = "\033[0m"
BOLD    = "\033[1m"
DIM     = "\033[2m"
RED     = "\033[31m"
GREEN   = "\033[32m"
YELLOW  = "\033[33m"
BLUE    = "\033[34m"
MAGENTA = "\033[35m"
CYAN    = "\033[36m"
WHITE   = "\033[37m"

# 색상 설정 (여기를 수정하면 그래프 색상이 바뀝니다)
AXIS_X_COLOR = DIM + RED       # x축: 어두운 빨간색
AXIS_Y_COLOR = DIM + GREEN     # y축: 어두운 초록색
FUNC_COLOR   = BOLD + CYAN     # 함수 곡선: 밝은 시안
LABEL_COLOR  = DIM + WHITE     # 레이블: 어두운 흰색

# 캔버스 크기 설정
CANVAS_W = 120   # 점 단위 가로 (점자 문자 60개)
CANVAS_H = 48    # 점 단위 세로 (점자 문자 12행)


# ─── 수식 안전 평가 ───
MATH_NAMESPACE = {
    "__builtins__": {},
    # 삼각함수
    "sin": math.sin, "cos": math.cos, "tan": math.tan,
    "asin": math.asin, "acos": math.acos, "atan": math.atan,
    "atan2": math.atan2,
    # 쌍곡선함수
    "sinh": math.sinh, "cosh": math.cosh, "tanh": math.tanh,
    "asinh": math.asinh, "acosh": math.acosh, "atanh": math.atanh,
    # 지수/로그
    "exp": math.exp, "log": math.log, "log2": math.log2, "log10": math.log10,
    "sqrt": math.sqrt, "cbrt": lambda x: math.copysign(abs(x) ** (1/3), x),
    # 기타
    "abs": abs, "floor": math.floor, "ceil": math.ceil,
    "pow": pow, "sign": lambda x: (1 if x > 0 else (-1 if x < 0 else 0)),
    # 상수
    "pi": math.pi, "e": math.e, "inf": math.inf,
}

def safe_eval_2d(expr, x):
    """2D 수식 평가: x를 변수로 사용"""
    ns = {**MATH_NAMESPACE, "x": x}
    try:
        result = eval(expr, ns)
        if isinstance(result, (int, float)) and math.isfinite(result):
            return result
    except:
        pass
    return None

def safe_eval_3d(expr, x, y):
    """3D 수식 평가: x, y를 변수로 사용"""
    ns = {**MATH_NAMESPACE, "x": x, "y": y}
    try:
        result = eval(expr, ns)
        if isinstance(result, (int, float)) and math.isfinite(result):
            return result
    except:
        pass
    return None


# ─── 점자 캔버스 (레이어별 색상 지원) ───
LAYER_NONE = 0
LAYER_AXIS_X = 1
LAYER_AXIS_Y = 2
LAYER_FUNC = 3

class BrailleCanvas:
    def __init__(self, w, h):
        self.w = w
        self.h = h
        self.pixels = [[LAYER_NONE] * w for _ in range(h)]

    def set(self, x, y, layer=LAYER_FUNC):
        ix, iy = int(round(x)), int(round(y))
        if 0 <= ix < self.w and 0 <= iy < self.h:
            if layer >= self.pixels[iy][ix]:
                self.pixels[iy][ix] = layer

    def get(self, x, y):
        if 0 <= x < self.w and 0 <= y < self.h:
            return self.pixels[y][x]
        return LAYER_NONE

    def line(self, x1, y1, x2, y2, layer=LAYER_FUNC):
        """브레젠햄 직선 알고리즘"""
        x1, y1, x2, y2 = int(round(x1)), int(round(y1)), int(round(x2)), int(round(y2))
        dx = abs(x2 - x1)
        dy = -abs(y2 - y1)
        sx = 1 if x1 < x2 else -1
        sy = 1 if y1 < y2 else -1
        err = dx + dy
        while True:
            self.set(x1, y1, layer)
            if x1 == x2 and y1 == y2:
                break
            e2 = 2 * err
            if e2 >= dy:
                err += dy
                x1 += sx
            if e2 <= dx:
                err += dx
                y1 += sy

    def render(self):
        """캔버스를 ANSI 컬러 점자 문자열로 변환"""
        LAYER_COLORS = {
            LAYER_AXIS_X: AXIS_X_COLOR,
            LAYER_AXIS_Y: AXIS_Y_COLOR,
            LAYER_FUNC:   FUNC_COLOR,
        }
        # 점자 패턴 내 각 점의 비트 오프셋
        OFFSETS = [
            (0, 0, 0x01), (0, 1, 0x02), (0, 2, 0x04),
            (1, 0, 0x08), (1, 1, 0x10), (1, 2, 0x20),
            (0, 3, 0x40), (1, 3, 0x80),
        ]
        lines = []
        for y in range(0, self.h, 4):
            line = ""
            for x in range(0, self.w, 2):
                pattern = 0
                max_layer = LAYER_NONE
                for dx, dy, bit in OFFSETS:
                    layer = self.get(x + dx, y + dy)
                    if layer > LAYER_NONE:
                        pattern |= bit
                        if layer > max_layer:
                            max_layer = layer
                char = chr(0x2800 + pattern)
                color = LAYER_COLORS.get(max_layer, "")
                if color:
                    line += color + char + RESET
                else:
                    line += char
            lines.append(line)
        return "\n".join(lines)


# ─── 2D 그래프 렌더링 ───
def render_2d(expr, x_min=-5.0, x_max=5.0):
    bc = BrailleCanvas(CANVAS_W, CANVAS_H)
    cx = CANVAS_W // 2   # 원점 x (픽셀)
    cy = CANVAS_H // 2   # 원점 y (픽셀)
    scale_x = CANVAS_W / (x_max - x_min)
    scale_y = scale_x     # 정사각 비율

    # x축 그리기
    if 0 <= cy < CANVAS_H:
        for px in range(CANVAS_W):
            bc.set(px, cy, LAYER_AXIS_X)
    # y축 그리기
    if 0 <= cx < CANVAS_W:
        for py in range(CANVAS_H):
            bc.set(cx, py, LAYER_AXIS_Y)

    # 함수 곡선 그리기
    prev_px, prev_py = None, None
    for px in range(CANVAS_W):
        x = x_min + (px / CANVAS_W) * (x_max - x_min)
        y = safe_eval_2d(expr, x)
        if y is None:
            prev_px, prev_py = None, None
            continue
        py = cy - y * scale_y
        if 0 <= py < CANVAS_H:
            if prev_px is not None and prev_py is not None:
                # 너무 큰 점프는 선으로 잇지 않음 (불연속 처리)
                if abs(py - prev_py) < CANVAS_H * 0.8:
                    bc.line(prev_px, prev_py, px, py, LAYER_FUNC)
                else:
                    bc.set(px, int(py), LAYER_FUNC)
            else:
                bc.set(px, int(py), LAYER_FUNC)
            prev_px, prev_py = px, py
        else:
            prev_px, prev_py = None, None

    return bc.render()


# ─── 3D 그래프 렌더링 (등축 투영 와이어프레임) ───
def render_3d(expr, x_range=(-3.0, 3.0), y_range=(-3.0, 3.0), grid_n=25):
    bc = BrailleCanvas(CANVAS_W, CANVAS_H)
    cx = CANVAS_W // 2
    cy = CANVAS_H // 2

    x_min, x_max = x_range
    y_min, y_max = y_range

    # 그리드 샘플링
    grid = []
    z_values = []
    for i in range(grid_n):
        row = []
        for j in range(grid_n):
            x = x_min + (x_max - x_min) * i / (grid_n - 1)
            y = y_min + (y_max - y_min) * j / (grid_n - 1)
            z = safe_eval_3d(expr, x, y)
            row.append((x, y, z))
            if z is not None:
                z_values.append(z)
        grid.append(row)

    if not z_values:
        return "수식을 평가할 수 없습니다."

    z_min = min(z_values)
    z_max = max(z_values)
    z_span = z_max - z_min if z_max != z_min else 1.0

    # 등축 투영 (isometric): 3D → 2D
    # 각도: x축 30도, y축 -30도
    cos30 = math.cos(math.radians(30))
    sin30 = math.sin(math.radians(30))
    scale = min(CANVAS_W, CANVAS_H) * 0.3 / max(x_max - x_min, y_max - y_min, z_span)

    def project(x, y, z):
        if z is None:
            return None, None
        # 중심 기준 정규화
        nx = x - (x_min + x_max) / 2
        ny = y - (y_min + y_max) / 2
        nz = (z - (z_min + z_max) / 2) 
        # 등축 투영
        sx = (nx - ny) * cos30 * scale + cx
        sy = -(nx + ny) * sin30 * scale - nz * scale + cy
        return sx, sy

    # x 방향 축 그리기
    for py in range(CANVAS_H):
        bc.set(cx, py, LAYER_AXIS_Y)

    # 와이어프레임 그리기 (x 방향 라인)
    for i in range(grid_n):
        prev_sx, prev_sy = None, None
        for j in range(grid_n):
            x, y, z = grid[i][j]
            sx, sy = project(x, y, z)
            if sx is not None and 0 <= sx < CANVAS_W and 0 <= sy < CANVAS_H:
                if prev_sx is not None:
                    bc.line(int(prev_sx), int(prev_sy), int(sx), int(sy), LAYER_FUNC)
                prev_sx, prev_sy = sx, sy
            else:
                prev_sx, prev_sy = None, None

    # 와이어프레임 그리기 (y 방향 라인)
    for j in range(grid_n):
        prev_sx, prev_sy = None, None
        for i in range(grid_n):
            x, y, z = grid[i][j]
            sx, sy = project(x, y, z)
            if sx is not None and 0 <= sx < CANVAS_W and 0 <= sy < CANVAS_H:
                if prev_sx is not None:
                    bc.line(int(prev_sx), int(prev_sy), int(sx), int(sy), LAYER_FUNC)
                prev_sx, prev_sy = sx, sy
            else:
                prev_sx, prev_sy = None, None

    return bc.render()


# ─── 메인 진입점 ───
def main():
    # 인자 로드
    if len(sys.argv) < 2:
        print("사용법: plot_function.py <args.json>")
        return

    with open(sys.argv[1], 'r') as f:
        args = json.load(f)

    expr = args.get("expression", "sin(x)")
    title = args.get("title", "")
    mode = args.get("mode", "2d")  # "2d" 또는 "3d"

    # 수식 전처리: ^ → ** (거듭제곱 변환)
    expr = expr.replace("^", "**")

    # 3D 여부 자동 감지: 수식에 y가 포함되면 3D
    if mode == "3d" or ("y" in expr and "y" not in ("hypot",)):
        # y가 변수로 사용된 경우 3D
        has_y_var = False
        for i, ch in enumerate(expr):
            if ch == 'y':
                # y가 함수 이름의 일부가 아닌지 확인
                before = expr[i-1] if i > 0 else ' '
                after = expr[i+1] if i+1 < len(expr) else ' '
                if not before.isalpha() and not after.isalpha():
                    has_y_var = True
                    break
        if has_y_var or mode == "3d":
            graph = render_3d(expr)
            dim_label = "3D"
        else:
            graph = render_2d(expr)
            dim_label = "2D"
    else:
        graph = render_2d(expr)
        dim_label = "2D"

    # 출력
    header = f"{LABEL_COLOR}── {dim_label}: {expr}"
    if title:
        header += f" ({title})"
    header += f" ──{RESET}"
    
    print(header)
    print(graph)
    print(f"{LABEL_COLOR}── {AXIS_X_COLOR}━ x축{RESET}  {AXIS_Y_COLOR}┃ y축{RESET}  {FUNC_COLOR}● f(x){RESET} {LABEL_COLOR}──{RESET}")


if __name__ == "__main__":
    main()
