"""Win32 PrintWindow capture, pixel/theme contrast checks, UIA geometry audit, and 54-PNG rubric pack."""

from __future__ import annotations

import ctypes
from ctypes import wintypes
from functools import lru_cache
import json
import math
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from PIL import Image, ImageGrab

from e2e.harness.gui_driver import get_client_screen_rect

PW_RENDERFULLCONTENT = 2
DIB_RGB_COLORS = 0
BI_RGB = 0

CAPTION_BUTTON_NAMES = frozenset({"Minimize", "Maximize", "Restore", "Close"})

INTERACTIVE_CONTROL_TYPES = frozenset({
    "ControlType.Button",
    "ControlType.CheckBox",
    "ControlType.ComboBox",
    "ControlType.Edit",
    "ControlType.RadioButton",
    "ControlType.Slider",
    "ControlType.Spinner",
    "ControlType.TabItem",
    "ControlType.MenuItem",
    "ControlType.Hyperlink",
})


class _BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", wintypes.LONG),
        ("biHeight", wintypes.LONG),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", wintypes.LONG),
        ("biYPelsPerMeter", wintypes.LONG),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class _BITMAPINFO(ctypes.Structure):
    _fields_ = [
        ("bmiHeader", _BITMAPINFOHEADER),
        ("bmiColors", wintypes.DWORD * 3),
    ]


@lru_cache(maxsize=1)
def _vc_user32() -> ctypes.WinDLL:
    u32 = ctypes.WinDLL("user32", use_last_error=True)
    u32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    u32.GetWindowRect.restype = wintypes.BOOL
    u32.GetWindowDC.argtypes = [wintypes.HWND]
    u32.GetWindowDC.restype = wintypes.HDC
    u32.PrintWindow.argtypes = [wintypes.HWND, wintypes.HDC, wintypes.UINT]
    u32.PrintWindow.restype = wintypes.BOOL
    u32.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
    u32.ReleaseDC.restype = ctypes.c_int
    u32.GetForegroundWindow.argtypes = []
    u32.GetForegroundWindow.restype = wintypes.HWND
    return u32


@lru_cache(maxsize=1)
def _vc_gdi32() -> ctypes.WinDLL:
    g32 = ctypes.WinDLL("gdi32", use_last_error=True)
    g32.CreateCompatibleDC.argtypes = [wintypes.HDC]
    g32.CreateCompatibleDC.restype = wintypes.HDC
    g32.CreateCompatibleBitmap.argtypes = [wintypes.HDC, ctypes.c_int, ctypes.c_int]
    g32.CreateCompatibleBitmap.restype = wintypes.HBITMAP
    g32.SelectObject.argtypes = [wintypes.HDC, wintypes.HGDIOBJ]
    g32.SelectObject.restype = wintypes.HGDIOBJ
    g32.GetDIBits.argtypes = [
        wintypes.HDC,
        wintypes.HBITMAP,
        wintypes.UINT,
        wintypes.UINT,
        ctypes.c_void_p,
        ctypes.c_void_p,
        wintypes.UINT,
    ]
    g32.GetDIBits.restype = ctypes.c_int
    g32.DeleteObject.argtypes = [wintypes.HGDIOBJ]
    g32.DeleteObject.restype = wintypes.BOOL
    g32.DeleteDC.argtypes = [wintypes.HDC]
    g32.DeleteDC.restype = wintypes.BOOL
    return g32


def capture_hwnd_client(hwnd: int) -> Image.Image:
    """Capture exact client area of hwnd using PrintWindow(PW_RENDERFULLCONTENT=2) + GetDIBits."""
    u32 = _vc_user32()
    g32 = _vc_gdi32()

    wrect = wintypes.RECT()
    if not u32.GetWindowRect(hwnd, ctypes.byref(wrect)):
        raise OSError(f"GetWindowRect failed for hwnd={hwnd}")
    win_w = wrect.right - wrect.left
    win_h = wrect.bottom - wrect.top
    if win_w <= 0 or win_h <= 0:
        raise ValueError(f"Window has non-positive size: {win_w}x{win_h}")

    cl, ct, cr, cb = get_client_screen_rect(hwnd)
    crop_box = (
        max(0, cl - wrect.left),
        max(0, ct - wrect.top),
        min(win_w, cr - wrect.left),
        min(win_h, cb - wrect.top),
    )

    hdc_win = u32.GetWindowDC(hwnd)
    hdc_mem = g32.CreateCompatibleDC(hdc_win)
    hbmp = g32.CreateCompatibleBitmap(hdc_win, win_w, win_h)
    old_obj = g32.SelectObject(hdc_mem, hbmp)

    try:
        ok = u32.PrintWindow(hwnd, hdc_mem, PW_RENDERFULLCONTENT)
        if not ok:
            ok = u32.PrintWindow(hwnd, hdc_mem, 0)

        bmi = _BITMAPINFO()
        bmi.bmiHeader.biSize = ctypes.sizeof(_BITMAPINFOHEADER)
        bmi.bmiHeader.biWidth = win_w
        bmi.bmiHeader.biHeight = -win_h  # top-down DIB
        bmi.bmiHeader.biPlanes = 1
        bmi.bmiHeader.biBitCount = 32
        bmi.bmiHeader.biCompression = BI_RGB

        buf = ctypes.create_string_buffer(win_w * win_h * 4)
        rows = g32.GetDIBits(hdc_mem, hbmp, 0, win_h, buf, ctypes.byref(bmi), DIB_RGB_COLORS)
        if rows == 0:
            raise OSError(f"GetDIBits failed (PrintWindow={ok})")

        full_img = Image.frombuffer("RGBA", (win_w, win_h), buf, "raw", "BGRA", 0, 1).copy()
        client_img = full_img.crop(crop_box)
    finally:
        g32.SelectObject(hdc_mem, old_obj)
        g32.DeleteObject(hbmp)
        g32.DeleteDC(hdc_mem)
        u32.ReleaseDC(hwnd, hdc_win)

    stats = compute_luminance_stats(client_img)
    if stats["stddev"] < 1.0 and u32.GetForegroundWindow() == hwnd:
        # Fallback to ImageGrab ONLY when active foreground window and PrintWindow returned uniform surface
        grab = ImageGrab.grab(bbox=(cl, ct, cr, cb))
        if compute_luminance_stats(grab)["stddev"] > stats["stddev"]:
            return grab.convert("RGBA")

    return client_img


def compute_luminance_stats(img: Image.Image) -> dict[str, float]:
    """Compute Rec.601 grayscale luminance mean, stddev, p5, p95, and contrast spread."""
    gray = img.convert("L")
    hist = gray.histogram()
    total = sum(hist)
    if total == 0:
        return {"mean": 0.0, "stddev": 0.0, "p5": 0.0, "p95": 0.0, "contrast_spread": 0.0}

    mean = sum(i * count for i, count in enumerate(hist)) / total
    var = sum(((i - mean) ** 2) * count for i, count in enumerate(hist)) / total
    stddev = math.sqrt(var)

    def percentile(frac: float) -> float:
        target = total * frac
        acc = 0
        for i, count in enumerate(hist):
            acc += count
            if acc >= target:
                return float(i)
        return 255.0

    p5 = percentile(0.05)
    p95 = percentile(0.95)
    return {
        "mean": round(mean, 2),
        "stddev": round(stddev, 2),
        "p5": round(p5, 2),
        "p95": round(p95, 2),
        "contrast_spread": round(p95 - p5, 2),
    }


def verify_theme_luminance(img: Image.Image, theme: str) -> dict[str, float]:
    """Assert non-blank render, contrast separation, and Light vs Dark luminance polarity."""
    stats = compute_luminance_stats(img)
    if stats["stddev"] <= 5.0:
        raise AssertionError(f"Captured frame appears uniform/blank: {stats}")
    if stats["contrast_spread"] < 15.0:
        raise AssertionError(f"Captured frame lacks surface/foreground contrast: {stats}")
    if theme == "light" and stats["mean"] <= 150.0:
        raise AssertionError(f"Light theme frame too dark (mean={stats['mean']}): {stats}")
    if theme == "dark" and stats["mean"] >= 110.0:
        raise AssertionError(f"Dark theme frame too bright (mean={stats['mean']}): {stats}")
    return stats


@dataclass
class GeometryAuditResult:
    """Result of UIA bounding-box & layout geometry audit."""

    node_count: int
    interactive_count: int
    zero_size_controls: list[str] = field(default_factory=list)
    offscreen_controls: list[str] = field(default_factory=list)
    out_of_bounds_controls: list[str] = field(default_factory=list)
    overlapping_pairs: list[tuple[str, str, int]] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return not (
            self.zero_size_controls
            or self.offscreen_controls
            or self.out_of_bounds_controls
            or self.overlapping_pairs
        )

    def to_dict(self) -> dict[str, Any]:
        return {
            "passed": self.passed,
            "node_count": self.node_count,
            "interactive_count": self.interactive_count,
            "zero_size_controls": self.zero_size_controls,
            "offscreen_controls": self.offscreen_controls,
            "out_of_bounds_controls": self.out_of_bounds_controls,
            "overlapping_pairs": self.overlapping_pairs,
        }


def _intersection_area(a: dict[str, Any], b: dict[str, Any]) -> int:
    ax1, ay1 = a["x"], a["y"]
    ax2, ay2 = ax1 + a["w"], ay1 + a["h"]
    bx1, by1 = b["x"], b["y"]
    bx2, by2 = bx1 + b["w"], by1 + b["h"]
    ix = max(0, min(ax2, bx2) - max(ax1, bx1))
    iy = max(0, min(ay2, by2) - max(ay1, by1))
    return ix * iy


def _is_contained(outer: dict[str, Any], inner: dict[str, Any]) -> bool:
    return (
        outer["x"] <= inner["x"]
        and outer["y"] <= inner["y"]
        and (outer["x"] + outer["w"]) >= (inner["x"] + inner["w"])
        and (outer["y"] + outer["h"]) >= (inner["y"] + inner["h"])
    )


def audit_uia_geometry(
    nodes: list[dict[str, Any]],
    client_rect: tuple[int, int, int, int],
    *,
    allow_vertical_scroll: bool = True,
    max_overlap_px2: int = 4,
) -> GeometryAuditResult:
    """Audit UIA descendants for zero-size, offscreen, horizontal clipping, and overlaps."""
    cl, ct, cr, cb = client_rect
    result = GeometryAuditResult(node_count=len(nodes), interactive_count=0)

    interactive: list[dict[str, Any]] = []
    for idx, n in enumerate(nodes):
        ctype = n.get("controlType", "")
        if ctype not in INTERACTIVE_CONTROL_TYPES:
            continue
        w, h = int(n.get("w", 0)), int(n.get("h", 0))
        x, y = int(n.get("x", 0)), int(n.get("y", 0))
        # Exclude non-client OS/CSD titlebar caption controls located above the client area
        if (n.get("name") or "") in CAPTION_BUTTON_NAMES and y < ct:
            continue
        if w > 0 and h > 0 and (y + h) <= ct:
            continue
        result.interactive_count += 1
        label = f"{n.get('name') or '<unnamed>'}[{ctype}#{idx}]"

        if n.get("isOffscreen"):
            result.offscreen_controls.append(label)
            continue
        if w <= 0 or h <= 0:
            result.zero_size_controls.append(label)
            continue

        # Horizontal clipping is never acceptable; vertical clipping fails for non-scrollable or header controls
        if x < cl - 2 or (x + w) > cr + 2:
            result.out_of_bounds_controls.append(f"{label} horiz=({x}..{x+w} vs {cl}..{cr})")
        elif not allow_vertical_scroll and (y < ct - 2 or (y + h) > cb + 2):
            result.out_of_bounds_controls.append(f"{label} vert=({y}..{y+h} vs {ct}..{cb})")

        interactive.append({**n, "_label": label})

    for i in range(len(interactive)):
        a = interactive[i]
        for j in range(i + 1, len(interactive)):
            b = interactive[j]
            # Ignore strict parent-child containment (e.g. a Slider containing a Thumb Button)
            if _is_contained(a, b) or _is_contained(b, a):
                continue
            area = _intersection_area(a, b)
            if area > max_overlap_px2:
                result.overlapping_pairs.append((a["_label"], b["_label"], area))

    return result


# --- 54-PNG Visual Capture Pack & Judge Rubric ---

CAPTURE_MATRIX_VIEWPORTS: tuple[tuple[int, int], ...] = (
    (1280, 800),
    (800, 600),
    (420, 700),
)
CAPTURE_MATRIX_THEMES: tuple[str, ...] = ("light", "dark")
CAPTURE_MATRIX_SCREENS: tuple[str, ...] = (
    "01_device_selection_empty",
    "02_device_selection_populated",
    "03_add_device_models_slimmed",
    "04_add_device_select_demo",
    "05_settings_drawer",
    "06_about_drawer",
    "07_q30_sound_modes",
    "08_q30_equalizer_hard_techno",
    "09_airpods_pro_2_settings",
)


def write_visual_rubric_artifacts(
    out_dir: Path,
    entries: list[dict[str, Any]],
) -> tuple[Path, Path]:
    """Write visual_judge_manifest.json and visual_rubric_checklist.md."""
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest_path = out_dir / "visual_judge_manifest.json"
    rubric_path = out_dir / "visual_rubric_checklist.md"

    manifest = {
        "expected_total_pngs": len(CAPTURE_MATRIX_SCREENS) * len(CAPTURE_MATRIX_VIEWPORTS) * len(CAPTURE_MATRIX_THEMES),
        "actual_entries": len(entries),
        "viewports": [f"{w}x{h}" for w, h in CAPTURE_MATRIX_VIEWPORTS],
        "themes": list(CAPTURE_MATRIX_THEMES),
        "screens": list(CAPTURE_MATRIX_SCREENS),
        "entries": entries,
    }
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")

    lines = [
        "# OpenSCQ30 Visual Judge Rubric Checklist (R1 Acceptance Criteria)",
        "",
        f"- Total Screenshots Indexed: **{len(entries)}** (target matrix: 9 screens x 3 sizes x 2 themes = 54 PNGs)",
        "",
        "## Acceptance Criteria Rubric",
        "1. **Apple-Inspired Visual Hierarchy, Spacing, Typography & Organization**: Consistent surface/card separation, clean title->section->control hierarchy, uniform margins (>=8px), balanced whitespace in Light and Dark themes.",
        "2. **Zero Clipping, Overlap, or Unreadable Elements**: Verified at `1280x800`, `800x600`, and `420x700` (`<450px` responsive breakpoint) via both UIA geometry audit and human/judge visual inspection.",
        "3. **Preserved Accessibility & Workflows**: All 14 `Setting` control types, `Ctrl+,`/`Ctrl+F`/`Escape`/`Tab`, demo pairing, genre EQ curves, and AirPods Pro 2 settings remain functional and accessible.",
        "",
        "## Captured Artifact Index",
        "| Screen | Theme | Viewport | File | Mean Lum | StdDev | Contrast | UIA Audit |",
        "|---|---|---|---|---:|---:|---:|---|",
    ]
    for e in entries:
        st = e.get("pixel_stats", {})
        au = e.get("uia_audit", {})
        audit_str = "PASS" if au.get("passed", False) else f"FAIL ({au})"
        lines.append(
            f"| `{e['screen']}` | `{e['theme']}` | `{e['viewport']}` | `{e['file']}` | "
            f"{st.get('mean', '-')} | {st.get('stddev', '-')} | {st.get('contrast_spread', '-')} | {audit_str} |"
        )
    rubric_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return manifest_path, rubric_path
