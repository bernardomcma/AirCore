"""Windows GUI process, AppData isolation, Win32 sizing/input, and UIA driver."""

from __future__ import annotations

import atexit
import base64
import ctypes
from ctypes import wintypes
from functools import lru_cache
import json
import os
import shutil
import sqlite3
import subprocess
import tempfile
import time
import tomllib
from pathlib import Path
from typing import Any

from e2e.harness.cli_driver import IsolatedCliSession, ensure_gui_built, find_gui_binary


# --- Win32 constants & structures ---
GWL_STYLE = -16
GWL_EXSTYLE = -20
SWP_NOZORDER = 0x0004
SWP_NOACTIVATE = 0x0010
SWP_SHOWWINDOW = 0x0040
DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = ctypes.c_void_p(-4)

WM_CLOSE = 0x0010
WM_KEYDOWN = 0x0100
WM_KEYUP = 0x0101
WM_CHAR = 0x0102
WM_LBUTTONDOWN = 0x0201
WM_LBUTTONUP = 0x0202
MK_LBUTTON = 0x0001

VK_TAB = 0x09
VK_BACK = 0x08
VK_RETURN = 0x0D
VK_CONTROL = 0x11
VK_ESCAPE = 0x1B
VK_A = 0x41
VK_OEM_COMMA = 0xBC
VK_F = 0x46

KEYEVENTF_KEYUP = 0x0002
INPUT_KEYBOARD = 1


class _KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", wintypes.WORD),
        ("wScan", wintypes.WORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.c_size_t),
    ]


class _INPUT_UNION(ctypes.Union):
    _fields_ = [("ki", _KEYBDINPUT), ("_pad", ctypes.c_byte * 32)]


class _INPUT(ctypes.Structure):
    _fields_ = [("type", wintypes.DWORD), ("union", _INPUT_UNION)]


WNDENUMPROC = (
    ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    if os.name == "nt"
    else None
)


@lru_cache(maxsize=1)
def _user32() -> ctypes.WinDLL:
    u32 = ctypes.WinDLL("user32", use_last_error=True)
    u32.SetProcessDpiAwarenessContext.argtypes = [ctypes.c_void_p]
    u32.SetProcessDpiAwarenessContext.restype = wintypes.BOOL
    u32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    u32.GetWindowThreadProcessId.restype = wintypes.DWORD
    u32.IsWindowVisible.argtypes = [wintypes.HWND]
    u32.IsWindowVisible.restype = wintypes.BOOL
    u32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
    u32.GetWindowTextLengthW.restype = ctypes.c_int
    u32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    u32.GetWindowTextW.restype = ctypes.c_int
    u32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    u32.GetWindowRect.restype = wintypes.BOOL
    if WNDENUMPROC is not None:
        u32.EnumWindows.argtypes = [WNDENUMPROC, wintypes.LPARAM]
    u32.EnumWindows.restype = wintypes.BOOL
    u32.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    u32.GetClientRect.restype = wintypes.BOOL
    u32.ClientToScreen.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.POINT)]
    u32.ClientToScreen.restype = wintypes.BOOL
    u32.ScreenToClient.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.POINT)]
    u32.ScreenToClient.restype = wintypes.BOOL
    u32.GetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int]
    u32.GetWindowLongW.restype = wintypes.LONG
    u32.GetDpiForWindow.argtypes = [wintypes.HWND]
    u32.GetDpiForWindow.restype = wintypes.UINT
    u32.AdjustWindowRectExForDpi.argtypes = [
        ctypes.POINTER(wintypes.RECT),
        wintypes.DWORD,
        wintypes.BOOL,
        wintypes.DWORD,
        wintypes.UINT,
    ]
    u32.AdjustWindowRectExForDpi.restype = wintypes.BOOL
    u32.SetWindowPos.argtypes = [
        wintypes.HWND,
        wintypes.HWND,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        wintypes.UINT,
    ]
    u32.SetWindowPos.restype = wintypes.BOOL
    u32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
    u32.PostMessageW.restype = wintypes.BOOL
    u32.SetForegroundWindow.argtypes = [wintypes.HWND]
    u32.SetForegroundWindow.restype = wintypes.BOOL
    u32.GetForegroundWindow.argtypes = []
    u32.GetForegroundWindow.restype = wintypes.HWND
    u32.SendInput.argtypes = [wintypes.UINT, ctypes.POINTER(_INPUT), ctypes.c_int]
    u32.SendInput.restype = wintypes.UINT
    return u32


@lru_cache(maxsize=1)
def _shell32() -> ctypes.WinDLL:
    s32 = ctypes.WinDLL("shell32", use_last_error=True)
    s32.SHGetKnownFolderPath.argtypes = [
        ctypes.c_void_p,
        wintypes.DWORD,
        wintypes.HANDLE,
        ctypes.POINTER(ctypes.c_void_p),
    ]
    s32.SHGetKnownFolderPath.restype = ctypes.c_long
    return s32


@lru_cache(maxsize=1)
def _ole32() -> ctypes.WinDLL:
    o32 = ctypes.WinDLL("ole32", use_last_error=True)
    o32.CoTaskMemFree.argtypes = [ctypes.c_void_p]
    o32.CoTaskMemFree.restype = None
    return o32


def get_roaming_appdata_dir() -> Path:
    """Resolve Windows FOLDERID_RoamingAppData via SHGetKnownFolderPath (matches dirs::config_dir)."""
    if os.name != "nt":
        return Path.home() / ".config"
    # {3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}
    guid = (ctypes.c_ubyte * 16)(
        0xDB, 0x85, 0xB6, 0x3E, 0xF9, 0x65, 0xF6, 0x4C,
        0xA0, 0x3A, 0xE3, 0xEF, 0x65, 0x72, 0x9F, 0x3D,
    )
    ptr = ctypes.c_void_p()
    hr = _shell32().SHGetKnownFolderPath(
        ctypes.byref(guid), 0, None, ctypes.byref(ptr)
    )
    if hr != 0 or not ptr.value:
        return Path.home() / "AppData" / "Roaming"
    try:
        path_str = ctypes.wstring_at(ptr.value)
    finally:
        _ole32().CoTaskMemFree(ptr)
    return Path(path_str)


def get_gui_config_dir() -> Path:
    """Return the config directory openscq30-gui.exe uses on Windows."""
    override = os.environ.get("OPENSCQ30_CONFIG_DIR")
    if override:
        return Path(override)
    return get_roaming_appdata_dir() / "openscq30"


def _rmtree_with_retry(path: Path, *, retries: int = 8, delay: float = 0.2) -> None:
    """Remove directory tree with brief retries for transient file locks; raise on persistent failure."""
    last_exc: Exception | None = None
    for attempt in range(retries):
        if not path.exists():
            return
        try:
            shutil.rmtree(path)
            return
        except OSError as exc:
            last_exc = exc
            if attempt + 1 < retries:
                time.sleep(delay)
    if path.exists() and last_exc is not None:
        raise last_exc


class AppDataBackupGuard:
    """Atomic snapshot, isolation, and crash-safe restore of %APPDATA%\\openscq30."""

    def __init__(self, target_dir: Path | None = None, sentinel_dir: Path | None = None) -> None:
        self.target_dir = target_dir or get_gui_config_dir()
        s_dir = sentinel_dir or (Path(tempfile.gettempdir()) / "openscq30_e2e_guard")
        s_dir.mkdir(parents=True, exist_ok=True)
        self.sentinel_path = s_dir / "appdata_sentinel.json"
        self.backup_root = s_dir / "backup_snapshot"
        self._had_original = False
        self._active = False
        self._launched_procs: list[subprocess.Popen[bytes]] = []

    def track_process(self, proc: subprocess.Popen[bytes]) -> None:
        self._launched_procs.append(proc)

    def recover_if_dirty(self) -> None:
        """If a previous run crashed before restoring, restore the original snapshot first."""
        if not self.sentinel_path.is_file():
            return
        meta = json.loads(self.sentinel_path.read_text(encoding="utf-8"))
        orig_target = Path(meta["target_dir"])
        bkp = Path(meta["backup_dir"])
        had_orig = bool(meta["had_original"])
        if orig_target.exists():
            _rmtree_with_retry(orig_target)
        if had_orig and bkp.exists():
            shutil.copytree(bkp, orig_target)
        if bkp.exists():
            _rmtree_with_retry(bkp)
        # Only remove sentinel AFTER restore + cleanup completely succeed
        self.sentinel_path.unlink(missing_ok=True)

    def __enter__(self) -> AppDataBackupGuard:
        self.recover_if_dirty()
        if self.backup_root.exists():
            _rmtree_with_retry(self.backup_root)

        self._had_original = self.target_dir.exists()
        if self._had_original:
            shutil.copytree(self.target_dir, self.backup_root)

        sentinel_payload = {
            "target_dir": str(self.target_dir),
            "backup_dir": str(self.backup_root),
            "had_original": self._had_original,
            "pid": os.getpid(),
            "created_at": time.time(),
        }
        self.sentinel_path.write_text(json.dumps(sentinel_payload, indent=2), encoding="utf-8")

        if self.target_dir.exists():
            _rmtree_with_retry(self.target_dir)
        self.target_dir.mkdir(parents=True, exist_ok=True)

        self._active = True
        atexit.register(self._emergency_restore)
        return self

    def _terminate_tracked(self) -> None:
        for proc in self._launched_procs:
            if proc.poll() is None:
                try:
                    proc.terminate()
                    proc.wait(timeout=3)
                except Exception:
                    try:
                        proc.kill()
                        proc.wait(timeout=2)
                    except Exception:
                        pass
        self._launched_procs.clear()

    def _emergency_restore(self) -> None:
        if self._active:
            self.__exit__(None, None, None)

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        if not self._active:
            return
        self._active = False
        try:
            atexit.unregister(self._emergency_restore)
        except Exception:
            pass
        self._terminate_tracked()
        if self.target_dir.exists():
            _rmtree_with_retry(self.target_dir)
        if self._had_original and self.backup_root.exists():
            shutil.copytree(self.backup_root, self.target_dir)
        if self.backup_root.exists():
            _rmtree_with_retry(self.backup_root)
        # Only unlink sentinel AFTER target_dir restore & backup_root cleanup succeed
        self.sentinel_path.unlink(missing_ok=True)


# --- Config TOML & SQLite Helpers ---

def write_gui_config(
    config_dir: Path,
    *,
    theme: str = "light",
    preferred_language: str | None = None,
) -> Path:
    """Seed openscq30-gui-config.toml in config_dir."""
    config_dir.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    if preferred_language is not None:
        escaped = preferred_language.replace('"', '\\"')
        lines.append(f'preferred_language = "{escaped}"')
    if theme is not None:
        lines.append(f'theme = "{theme}"')
    cfg_path = config_dir / "openscq30-gui-config.toml"
    cfg_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return cfg_path


def read_gui_config(config_dir: Path) -> dict[str, Any]:
    """Read openscq30-gui-config.toml using stdlib tomllib."""
    cfg_path = config_dir / "openscq30-gui-config.toml"
    if not cfg_path.is_file():
        return {}
    with cfg_path.open("rb") as f:
        return tomllib.load(f)


def write_legacy_v1_config_toml(
    config_dir: Path,
    custom_profiles: dict[str, list[int]],
    *,
    corrupt: bool = False,
) -> Path:
    """Seed legacy v1 config.toml matching gui/src/openscq30_v1_migration.rs LegacyConfig schema."""
    config_dir.mkdir(parents=True, exist_ok=True)
    legacy_path = config_dir / "config.toml"
    if corrupt:
        legacy_path.write_text("[[[invalid toml syntax ===\n", encoding="utf-8")
        return legacy_path
    lines: list[str] = []
    for name, bands in custom_profiles.items():
        escaped = name.replace("\\", "\\\\").replace('"', '\\"')
        arr = ", ".join(str(b) for b in bands)
        lines.append(f'[equalizer_custom_profiles."{escaped}"]')
        lines.append(f"volume_offsets = [{arr}]")
    legacy_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return legacy_path


def seed_paired_device(
    repo_root: Path,
    db_path: Path,
    mac: str,
    model: str,
    *,
    demo: bool = True,
) -> None:
    """Seed a paired device via public openscq30 CLI so SQLite schema/migrations are authentic."""
    with IsolatedCliSession(repo_root, db_path=db_path) as cli:
        cli.pair_device(mac, model, demo=demo)


def query_sqlite(db_path: Path, sql: str, params: tuple[Any, ...] = ()) -> list[tuple[Any, ...]]:
    """Read-only SQLite query helper for post-action DB verification."""
    conn = sqlite3.connect(str(db_path))
    try:
        cur = conn.execute(sql, params)
        return cur.fetchall()
    finally:
        conn.close()


def seed_quick_preset_sqlite(
    db_path: Path,
    device_model: str,
    name: str,
    fields: list[dict[str, Any]],
) -> None:
    """Insert/replace a quick_preset row using SQLite 3.45+ jsonb()."""
    payload = json.dumps(fields, separators=(",", ":"))
    conn = sqlite3.connect(str(db_path))
    try:
        conn.execute(
            """
            INSERT INTO quick_preset (device_model, name, fields)
            VALUES (?, ?, jsonb(?))
            ON CONFLICT(device_model, name) DO UPDATE SET fields = jsonb(?)
            """,
            (device_model, name, payload, payload),
        )
        conn.commit()
    finally:
        conn.close()


# --- Win32 Window & Input Helpers ---

def enable_per_monitor_dpi() -> None:
    """Set Per-Monitor V2 DPI awareness on current Python process."""
    if os.name != "nt":
        return
    try:
        _user32().SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
    except Exception:
        pass


def enum_windows_for_pid(pid: int) -> list[tuple[int, str]]:
    """Return list of (hwnd, title) for visible top-level windows belonging to pid."""
    u32 = _user32()
    results: list[tuple[int, str]] = []
    enum_proc_cls = WNDENUMPROC or ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def callback(hwnd: int, _lparam: int) -> bool:
        proc_id = wintypes.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(proc_id))
        if proc_id.value == pid and u32.IsWindowVisible(hwnd):
            length = u32.GetWindowTextLengthW(hwnd)
            buf = ctypes.create_unicode_buffer(length + 1)
            u32.GetWindowTextW(hwnd, buf, length + 1)
            rect = wintypes.RECT()
            u32.GetWindowRect(hwnd, ctypes.byref(rect))
            if (rect.right - rect.left) > 0 and (rect.bottom - rect.top) > 0:
                results.append((hwnd, buf.value))
        return True

    u32.EnumWindows(enum_proc_cls(callback), 0)
    return results


def wait_for_hwnd(
    pid: int,
    title_substring: str = "AirCore",
    timeout: float = 20.0,
    *,
    proc: subprocess.Popen[bytes] | None = None,
) -> int:
    """Wait until PID presents a visible non-zero window matching title_substring."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc is not None and proc.poll() is not None:
            raise RuntimeError(
                f"Process PID {pid} exited prematurely with code {proc.returncode} before presenting a window"
            )
        wins = enum_windows_for_pid(pid)
        for hwnd, title in wins:
            if not title_substring or title_substring.lower() in title.lower():
                return hwnd
        if wins and not title_substring:
            return wins[0][0]
        time.sleep(0.2)
    raise TimeoutError(f"Timed out after {timeout}s waiting for window of PID {pid}")


def get_window_title(hwnd: int) -> str:
    u32 = _user32()
    length = u32.GetWindowTextLengthW(hwnd)
    buf = ctypes.create_unicode_buffer(length + 1)
    u32.GetWindowTextW(hwnd, buf, length + 1)
    return buf.value


def get_client_screen_rect(hwnd: int) -> tuple[int, int, int, int]:
    """Return client area (left, top, right, bottom) in screen pixels."""
    u32 = _user32()
    crect = wintypes.RECT()
    if not u32.GetClientRect(hwnd, ctypes.byref(crect)):
        raise OSError(f"GetClientRect failed for hwnd={hwnd}")
    tl = wintypes.POINT(crect.left, crect.top)
    br = wintypes.POINT(crect.right, crect.bottom)
    u32.ClientToScreen(hwnd, ctypes.byref(tl))
    u32.ClientToScreen(hwnd, ctypes.byref(br))
    return (tl.x, tl.y, br.x, br.y)


def resize_client_window(hwnd: int, client_w: int, client_h: int, *, x: int = 40, y: int = 40) -> tuple[int, int]:
    """Resize HWND so GetClientRect matches (client_w, client_h) using AdjustWindowRectExForDpi."""
    enable_per_monitor_dpi()
    u32 = _user32()
    style = u32.GetWindowLongW(hwnd, GWL_STYLE) & 0xFFFFFFFF
    ex_style = u32.GetWindowLongW(hwnd, GWL_EXSTYLE) & 0xFFFFFFFF
    dpi = u32.GetDpiForWindow(hwnd) or 96
    rect = wintypes.RECT(0, 0, client_w, client_h)
    if not u32.AdjustWindowRectExForDpi(ctypes.byref(rect), style, False, ex_style, dpi):
        raise OSError(f"AdjustWindowRectExForDpi failed: err={ctypes.get_last_error()}")
    outer_w = rect.right - rect.left
    outer_h = rect.bottom - rect.top
    u32.SetWindowPos(hwnd, None, x, y, outer_w, outer_h, SWP_NOZORDER | SWP_SHOWWINDOW)
    time.sleep(0.35)
    crect = wintypes.RECT()
    u32.GetClientRect(hwnd, ctypes.byref(crect))
    actual = (crect.right - crect.left, crect.bottom - crect.top)
    return actual


def click_screen_point(hwnd: int, screen_x: int, screen_y: int) -> None:
    """Click a screen coordinate inside hwnd using PostMessageW WM_LBUTTONDOWN/UP."""
    u32 = _user32()
    pt = wintypes.POINT(int(screen_x), int(screen_y))
    u32.ScreenToClient(hwnd, ctypes.byref(pt))
    lparam = ((pt.y & 0xFFFF) << 16) | (pt.x & 0xFFFF)
    u32.PostMessageW(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, lparam)
    time.sleep(0.05)
    u32.PostMessageW(hwnd, WM_LBUTTONUP, 0, lparam)
    time.sleep(0.25)


def send_key(hwnd: int, vk: int, *, ctrl: bool = False) -> None:
    """Send key press to hwnd via SendInput if foreground, else PostMessageW."""
    u32 = _user32()
    fg_ok = False
    try:
        u32.SetForegroundWindow(hwnd)
        fg_ok = u32.GetForegroundWindow() == hwnd
    except Exception:
        fg_ok = False

    if fg_ok:
        inputs: list[_INPUT] = []
        if ctrl:
            inputs.append(_INPUT(type=INPUT_KEYBOARD, union=_INPUT_UNION(ki=_KEYBDINPUT(wVk=VK_CONTROL))))
        inputs.append(_INPUT(type=INPUT_KEYBOARD, union=_INPUT_UNION(ki=_KEYBDINPUT(wVk=vk))))
        inputs.append(_INPUT(type=INPUT_KEYBOARD, union=_INPUT_UNION(ki=_KEYBDINPUT(wVk=vk, dwFlags=KEYEVENTF_KEYUP))))
        if ctrl:
            inputs.append(_INPUT(type=INPUT_KEYBOARD, union=_INPUT_UNION(ki=_KEYBDINPUT(wVk=VK_CONTROL, dwFlags=KEYEVENTF_KEYUP))))
        arr = (_INPUT * len(inputs))(*inputs)
        u32.SendInput(len(inputs), arr, ctypes.sizeof(_INPUT))
    else:
        if ctrl:
            u32.PostMessageW(hwnd, WM_KEYDOWN, VK_CONTROL, 0)
        u32.PostMessageW(hwnd, WM_KEYDOWN, vk, 0)
        time.sleep(0.04)
        u32.PostMessageW(hwnd, WM_KEYUP, vk, 0xC0000000)
        if ctrl:
            u32.PostMessageW(hwnd, WM_KEYUP, VK_CONTROL, 0xC0000000)
    time.sleep(0.25)


def send_text(hwnd: int, text: str) -> None:
    """Send Unicode characters via WM_CHAR to hwnd."""
    u32 = _user32()
    for ch in text:
        u32.PostMessageW(hwnd, WM_CHAR, ord(ch), 0)
        time.sleep(0.02)
    time.sleep(0.2)


def close_window_cleanly(hwnd: int, proc: subprocess.Popen[bytes], timeout: float = 8.0) -> int:
    """Post WM_CLOSE and verify clean process exit."""
    _user32().PostMessageW(hwnd, WM_CLOSE, 0, 0)
    try:
        return proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.terminate()
        return proc.wait(timeout=3)


# --- Windows UI Automation Bridge (PowerShell UIAutomationClient) ---

def _run_ps_uia(script: str) -> Any:
    full_script = (
        "$ProgressPreference = 'SilentlyContinue'\n"
        "Add-Type -AssemblyName UIAutomationClient\n"
        "Add-Type -AssemblyName UIAutomationTypes\n"
        "$ErrorActionPreference = 'Stop'\n"
        + script
    )
    encoded = base64.b64encode(full_script.encode("utf-16-le")).decode("ascii")
    proc = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=25,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"PowerShell UIA bridge failed (rc={proc.returncode}): {proc.stderr}\n{proc.stdout}")
    out = proc.stdout.strip()
    return json.loads(out) if out else None


def uia_get_tree(hwnd: int) -> list[dict[str, Any]]:
    """Inspect all UIAutomation descendants of hwnd and return structured node dicts."""
    script = f"""
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]({int(hwnd)}))
$cond = [System.Windows.Automation.Condition]::TrueCondition
$all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
$list = @()
foreach ($el in $all) {{
    $cur = $el.Current
    $r = $cur.BoundingRectangle
    $pats = @($el.GetSupportedPatterns() | ForEach-Object {{ $_.ProgrammaticName }})
    $val = $null
    if ($pats -contains 'ValuePatternIdentifiers.Pattern') {{
        try {{
            $vp = $el.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
            $val = $vp.Current.Value
        }} catch {{}}
    }}
    $toggle = $null
    if ($pats -contains 'TogglePatternIdentifiers.Pattern') {{
        try {{
            $tp = $el.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
            $toggle = $tp.Current.ToggleState.ToString()
        }} catch {{}}
    }}
    $list += [PSCustomObject]@{{
        name = $cur.Name
        automationId = $cur.AutomationId
        controlType = $cur.ControlType.ProgrammaticName
        x = if ([double]::IsInfinity($r.X) -or [double]::IsNaN($r.X)) {{ 0 }} else {{ [int][Math]::Round($r.X) }}
        y = if ([double]::IsInfinity($r.Y) -or [double]::IsNaN($r.Y)) {{ 0 }} else {{ [int][Math]::Round($r.Y) }}
        w = if ([double]::IsInfinity($r.Width) -or [double]::IsNaN($r.Width)) {{ 0 }} else {{ [int][Math]::Round($r.Width) }}
        h = if ([double]::IsInfinity($r.Height) -or [double]::IsNaN($r.Height)) {{ 0 }} else {{ [int][Math]::Round($r.Height) }}
        isOffscreen = [bool]$cur.IsOffscreen
        isEnabled = [bool]$cur.IsEnabled
        hasKeyboardFocus = [bool]$cur.HasKeyboardFocus
        patterns = $pats
        value = $val
        toggleState = $toggle
    }}
}}
ConvertTo-Json -InputObject @($list) -Depth 4 -Compress
"""
    res = _run_ps_uia(script)
    if res is None:
        return []
    return res if isinstance(res, list) else [res]


def uia_find_nodes(
    nodes: list[dict[str, Any]],
    *,
    name: str | None = None,
    name_contains: str | None = None,
    control_type: str | None = None,
    visible_only: bool = False,
) -> list[dict[str, Any]]:
    """Filter UIA tree nodes by name, substring, controlType, and visibility."""
    out: list[dict[str, Any]] = []
    for n in nodes:
        if visible_only and (n.get("isOffscreen") or n.get("w", 0) <= 0 or n.get("h", 0) <= 0):
            continue
        if control_type and n.get("controlType") != control_type:
            continue
        n_name = n.get("name") or ""
        if name is not None and n_name != name:
            continue
        if name_contains is not None and name_contains.lower() not in n_name.lower():
            continue
        out.append(n)
    return out


def _uia_occurrence_index(nodes: list[dict[str, Any]], target: dict[str, Any]) -> int:
    """Return 0-based index of target among elements sharing the same (name, controlType)."""
    t_name = target.get("name") or ""
    t_ct = target.get("controlType") or ""
    occ = 0
    for n in nodes:
        if n is target:
            return occ
        if (n.get("name") or "") == t_name and (not t_ct or (n.get("controlType") or "") == t_ct):
            occ += 1
    raise RuntimeError(f"Target UIA node missing from tree snapshot: {target}")


def uia_invoke(
    hwnd: int,
    *,
    name: str | None = None,
    name_contains: str | None = None,
    control_type: str | None = None,
    index: int = 0,
    visible_only: bool = False,
    min_y: int | None = None,
) -> dict[str, Any]:
    """Invoke a UIA control via InvokePattern/TogglePattern/ExpandCollapse or center click."""
    nodes = uia_get_tree(hwnd)
    matches = uia_find_nodes(
        nodes,
        name=name,
        name_contains=name_contains,
        control_type=control_type,
        visible_only=visible_only,
    )
    if min_y is not None:
        matches = [n for n in matches if n.get("y", 0) >= min_y]
    if index < 0 or index >= len(matches):
        raise AssertionError(
            f"UIA control not found (name={name!r}, name_contains={name_contains!r}, "
            f"control_type={control_type!r}, index={index}, visible_only={visible_only}, min_y={min_y}); "
            f"available names={[n.get('name') for n in nodes if n.get('name')]}"
        )
    target = matches[index]
    if target.get("isEnabled") is False:
        raise AssertionError(f"Cannot invoke disabled UIA control: {target}")
    patterns = target.get("patterns") or []
    if "InvokePatternIdentifiers.Pattern" in patterns or "TogglePatternIdentifiers.Pattern" in patterns:
        ps_idx = _uia_occurrence_index(nodes, target)
        escaped = (target.get("name") or "").replace("'", "''")
        ct = (target.get("controlType") or "").replace("'", "''")
        script = f"""
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]({int(hwnd)}))
$all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
$idx = 0
foreach ($el in $all) {{
    $c = $el.Current
    if ($c.Name -ceq '{escaped}' -and ('{ct}' -eq '' -or $c.ControlType.ProgrammaticName -ceq '{ct}')) {{
        if ($idx -eq {ps_idx}) {{
            if ($c.IsEnabled -eq $false) {{
                [Console]::Error.WriteLine("Matched UIA element is disabled (name='{escaped}', ct='{ct}', idx={ps_idx})")
                exit 4
            }}
            $pats = @($el.GetSupportedPatterns() | ForEach-Object {{ $_.ProgrammaticName }})
            try {{
                if ($pats -contains 'InvokePatternIdentifiers.Pattern') {{
                    $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
                    'invoked' | ConvertTo-Json
                    exit 0
                }} elseif ($pats -contains 'TogglePatternIdentifiers.Pattern') {{
                    $el.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
                    'toggled' | ConvertTo-Json
                    exit 0
                }}
            }} catch [System.Windows.Automation.ElementNotEnabledException] {{
                [Console]::Error.WriteLine("ElementNotEnabledException invoking UIA element (name='{escaped}', ct='{ct}', idx={ps_idx})")
                exit 4
            }}
            [Console]::Error.WriteLine("Matched element lacked InvokePattern/TogglePattern")
            exit 3
        }}
        $idx++
    }}
}}
[Console]::Error.WriteLine("UIA element not found during invoke (name='{escaped}', ct='{ct}', idx={ps_idx})")
exit 2
"""
        try:
            res = _run_ps_uia(script)
            if res not in ("invoked", "toggled"):
                raise RuntimeError(f"Unexpected PowerShell UIA invoke result: {res!r}")
            time.sleep(0.25)
            return target
        except Exception as exc:
            msg_low = str(exc).lower()
            if "rc=4" in msg_low or "disabled" in msg_low or "elementnotenabled" in msg_low:
                raise
            if target.get("isOffscreen") or target.get("w", 0) <= 0 or target.get("h", 0) <= 0:
                raise
    if target.get("w", 0) <= 0 or target.get("h", 0) <= 0:
        raise AssertionError(f"Cannot click UIA target with non-positive bounds: {target}")
    cx = target["x"] + max(1, target["w"] // 2)
    cy = target["y"] + max(1, target["h"] // 2)
    click_screen_point(hwnd, cx, cy)
    return target


def uia_set_value(
    hwnd: int,
    text: str,
    *,
    name: str | None = None,
    name_contains: str | None = None,
    index: int = 0,
) -> dict[str, Any]:
    """Set text on an Edit control via ValuePattern or click + clear + WM_CHAR fallback."""
    nodes = uia_get_tree(hwnd)
    matches = uia_find_nodes(
        nodes,
        name=name,
        name_contains=name_contains,
        control_type="ControlType.Edit",
    )
    if not matches:
        matches = uia_find_nodes(nodes, name=name, name_contains=name_contains)
    if index < 0 or index >= len(matches):
        raise AssertionError(f"Edit control not found (name={name!r}, name_contains={name_contains!r})")
    target = matches[index]
    patterns = target.get("patterns") or []
    if "ValuePatternIdentifiers.Pattern" in patterns:
        ps_idx = _uia_occurrence_index(nodes, target)
        escaped = (target.get("name") or "").replace("'", "''")
        ct = (target.get("controlType") or "").replace("'", "''")
        b64_text = base64.b64encode(text.encode("utf-8")).decode("ascii")
        script = f"""
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]({int(hwnd)}))
$all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
$valText = [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String('{b64_text}'))
$idx = 0
foreach ($el in $all) {{
    $c = $el.Current
    if ($c.Name -ceq '{escaped}' -and ('{ct}' -eq '' -or $c.ControlType.ProgrammaticName -ceq '{ct}')) {{
        if ($idx -eq {ps_idx}) {{
            $pats = @($el.GetSupportedPatterns() | ForEach-Object {{ $_.ProgrammaticName }})
            if ($pats -contains 'ValuePatternIdentifiers.Pattern') {{
                $el.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($valText)
                'set' | ConvertTo-Json
                exit 0
            }}
            [Console]::Error.WriteLine("Matched Edit element lacked ValuePattern")
            exit 3
        }}
        $idx++
    }}
}}
[Console]::Error.WriteLine("UIA Edit element not found (name='{escaped}', ct='{ct}', idx={ps_idx})")
exit 2
"""
        try:
            res = _run_ps_uia(script)
            if res == "set":
                time.sleep(0.2)
                return target
        except Exception:
            pass
    cx = target["x"] + max(1, target["w"] // 2)
    cy = target["y"] + max(1, target["h"] // 2)
    click_screen_point(hwnd, cx, cy)
    send_key(hwnd, VK_A, ctrl=True)
    send_key(hwnd, VK_BACK)
    send_text(hwnd, text)
    return target


class GuiAppSession:
    """Context manager launching openscq30-gui.exe inside AppDataBackupGuard."""

    def __init__(
        self,
        repo_root: Path,
        *,
        theme: str = "light",
        preferred_language: str | None = None,
        seed_config: bool = True,
        extra_env: dict[str, str] | None = None,
        width: int = 1280,
        height: int = 800,
    ) -> None:
        self.repo_root = repo_root
        self.theme = theme
        self.preferred_language = preferred_language
        self.seed_config = seed_config
        self.extra_env = extra_env or {}
        self.width = width
        self.height = height
        self.guard = AppDataBackupGuard()
        self.config_dir = self.guard.target_dir
        self.db_path = self.config_dir / "database.sqlite"
        self.proc: subprocess.Popen[bytes] | None = None
        self.hwnd: int = 0
        self._stderr_path: Path | None = None

    def __enter__(self) -> GuiAppSession:
        self.guard.__enter__()
        if self.seed_config:
            write_gui_config(
                self.config_dir,
                theme=self.theme,
                preferred_language=self.preferred_language,
            )
        return self

    def launch(self) -> int:
        """Start openscq30-gui.exe, discover HWND, and resize client viewport."""
        exe = find_gui_binary(self.repo_root)
        if exe is None:
            exe = ensure_gui_built(self.repo_root)
        env = os.environ.copy()
        env.update(self.extra_env)
        stderr_file = self.config_dir / "_e2e_gui_stderr.log"
        self._stderr_path = stderr_file
        self._stderr_handle = open(stderr_file, "wb")
        self.proc = subprocess.Popen(
            [str(exe)],
            cwd=str(self.repo_root),
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=self._stderr_handle,
        )
        self.guard.track_process(self.proc)
        self.hwnd = wait_for_hwnd(self.proc.pid, "AirCore", timeout=20.0, proc=self.proc)
        resize_client_window(self.hwnd, self.width, self.height)
        time.sleep(0.5)
        return self.hwnd

    def close(self) -> int:
        if self.proc is None:
            return 0
        try:
            rc = close_window_cleanly(self.hwnd, self.proc)
        finally:
            if hasattr(self, "_stderr_handle") and not self._stderr_handle.closed:
                self._stderr_handle.close()
            self.proc = None
            self.hwnd = 0
        return rc

    def read_stderr(self) -> str:
        if self._stderr_path and self._stderr_path.is_file():
            return self._stderr_path.read_text(encoding="utf-8", errors="replace")
        return ""

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        try:
            if self.proc is not None and self.proc.poll() is None:
                self.close()
            elif hasattr(self, "_stderr_handle") and not self._stderr_handle.closed:
                self._stderr_handle.close()
        finally:
            self.guard.__exit__(exc_type, exc, tb)
