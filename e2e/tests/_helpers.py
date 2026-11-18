"""Shared opaque-box CLI & GUI workflow helpers for E2E tests."""

from __future__ import annotations

import json
import re
import time
from pathlib import Path
from typing import Any

from e2e.fixtures.aap_fixtures import (
    DEMO_APP2_MAC,
    DEMO_Q30_MAC,
    flatten_list_settings,
    resolve_app2_setting,
)
from e2e.harness.cli_driver import CommandResult, IsolatedCliSession
from e2e.harness.gui_driver import (
    GuiAppSession,
    click_screen_point,
    get_client_screen_rect,
    send_key,
    send_text,
    uia_find_nodes,
    uia_get_tree,
    uia_invoke,
    uia_set_value,
    VK_ESCAPE,
)
from e2e.harness.visual_capture import (
    audit_uia_geometry,
    capture_hwnd_client,
    verify_theme_luminance,
)


CARGO_TEST_OK_RE = re.compile(r"test result: ok\.\s+(\d+)\s+passed;\s+0 failed")
CARGO_TEST_FAIL_RE = re.compile(r"test result:\s+FAILED|;\s*[1-9]\d*\s+failed")
CARGO_TEST_LINE_RE = re.compile(r"^test\s+(\S+)\s+\.\.\.\s+ok\s*$", re.MULTILINE)


def assert_cargo_tests_passed(
    res: CommandResult,
    *,
    min_passed: int = 1,
    required_name_substrings: tuple[str, ...] | None = None,
    context: str = "cargo test",
) -> list[str]:
    """Assert cargo test succeeded with >= min_passed tests and optional name/namespace matching."""
    assert res.returncode == 0, f"{context} failed (rc={res.returncode}):\n{res.stdout}\n{res.stderr}"
    assert not CARGO_TEST_FAIL_RE.search(res.stdout), f"{context} reported failures:\n{res.stdout}"
    ok_counts = CARGO_TEST_OK_RE.findall(res.stdout)
    assert ok_counts, f"{context} missing passing 'test result: ok.' summary:\n{res.stdout}\n{res.stderr}"
    total_passed = sum(int(c) for c in ok_counts)
    assert total_passed >= min_passed, (
        f"{context} expected >= {min_passed} passed tests, got {total_passed}:\n{res.stdout}"
    )
    executed = CARGO_TEST_LINE_RE.findall(res.stdout)
    if required_name_substrings:
        needles = tuple(s.lower() for s in required_name_substrings)
        matched = [name for name in executed if any(sub in name.lower() for sub in needles)]
        assert matched, (
            f"{context} executed {total_passed} test(s) {executed}, "
            f"but none matched required substrings {required_name_substrings}"
        )
        return matched
    return executed


def parse_ftl_keys(ftl_path: Path) -> dict[str, str]:
    """Parse top-level key = value pairs from an English Fluent .ftl file."""
    text = ftl_path.read_text(encoding="utf-8")
    entries: dict[str, str] = {}
    for line in text.splitlines():
        if not line or line.startswith("#") or line[0].isspace():
            continue
        if "=" in line:
            k, v = line.split("=", 1)
            entries[k.strip()] = v.strip()
    return entries


def extract_setting_from_cli_json(
    setting_json: list[dict[str, Any]],
    setting_id: str,
) -> dict[str, Any]:
    """Extract a setting entry by case-insensitive settingId from `device setting --json` output."""
    for entry in setting_json:
        if entry.get("settingId", "").lower() == setting_id.lower():
            return entry
    raise AssertionError(
        f"Setting {setting_id!r} missing from CLI JSON output: {setting_json}"
    )


def get_eq_preset_meta(cli: IsolatedCliSession, mac: str) -> dict[str, Any]:
    """Fetch `presetEqualizerProfile` setting schema from `list-settings --json`."""
    flat = flatten_list_settings(cli.list_settings(mac))
    for item in flat:
        if item.get("settingId") == "presetEqualizerProfile":
            return item
    raise AssertionError(f"presetEqualizerProfile not found in settings for {mac}: {flat}")


def get_volume_adjustments_meta(cli: IsolatedCliSession, mac: str) -> dict[str, Any]:
    """Fetch `volumeAdjustments` setting schema from `list-settings --json`."""
    flat = flatten_list_settings(cli.list_settings(mac))
    for item in flat:
        if item.get("settingId") == "volumeAdjustments":
            return item
    raise AssertionError(f"volumeAdjustments not found in settings for {mac}: {flat}")


def require_app2_setting(flat: list[dict[str, Any]], canonical_key: str) -> dict[str, Any]:
    """Resolve an AirPods Pro 2 setting or fail loudly with available settingIds."""
    found = resolve_app2_setting(flat, canonical_key)
    if found is None:
        avail = [s.get("settingId") for s in flat]
        raise AssertionError(
            f"Expected AirPods Pro 2 setting '{canonical_key}' not found; available={avail}"
        )
    return found


def gui_click_button_matching(hwnd: int, label_substrings: tuple[str, ...]) -> dict[str, Any]:
    """Find and invoke the first visible client-area button/control matching any label substring."""
    nodes = uia_get_tree(hwnd)
    _, ct, _, _ = get_client_screen_rect(hwnd)
    for sub in label_substrings:
        hits = [
            h for h in uia_find_nodes(nodes, name_contains=sub, visible_only=True)
            if h.get("y", 0) >= ct
        ]
        # Prefer interactive controls over pure text nodes
        interactive = [
            h for h in hits if h.get("controlType") in {
                "ControlType.Button",
                "ControlType.CheckBox",
                "ControlType.TabItem",
                "ControlType.MenuItem",
                "ControlType.ListItem",
            }
        ]
        if interactive:
            target = interactive[0]
            same = [
                n for n in uia_find_nodes(
                    nodes,
                    name=target["name"],
                    control_type=target["controlType"],
                    visible_only=True,
                )
                if n.get("y", 0) >= ct
            ]
            idx = next((i for i, n in enumerate(same) if n is target), 0)
            return uia_invoke(
                hwnd,
                name=target["name"],
                control_type=target["controlType"],
                index=idx,
                visible_only=True,
                min_y=ct,
            )
        if hits:
            target = hits[0]
            cx = target["x"] + max(1, target["w"] // 2)
            cy = target["y"] + max(1, target["h"] // 2)
            click_screen_point(hwnd, cx, cy)
            return target
    avail = [(n.get("name"), n.get("controlType")) for n in nodes if n.get("name")]
    raise AssertionError(f"No UIA node matched {label_substrings}; available={avail}")


def gui_connect_device_by_label(hwnd: int, label_substrings: tuple[str, ...]) -> None:
    """Click the Connect button on the DeviceSelection row matching label_substrings."""
    nodes = uia_get_tree(hwnd)
    _, ct, _, _ = get_client_screen_rect(hwnd)
    connect_btns = [
        n for n in uia_find_nodes(nodes, name="Connect", control_type="ControlType.Button", visible_only=True)
        if n.get("y", 0) >= ct
    ]
    assert connect_btns, (
        f"No visible Connect buttons in client area; buttons={[(n.get('name'), n.get('y')) for n in nodes]}"
    )
    label_hits: list[dict[str, Any]] = []
    for sub in label_substrings:
        label_hits.extend(
            n for n in uia_find_nodes(nodes, name_contains=sub, visible_only=True)
            if n.get("y", 0) >= ct
        )
    assert label_hits, (
        f"Device label {label_substrings} not found on DeviceSelection; "
        f"names={[n.get('name') for n in nodes if n.get('name')]}"
    )
    ref = label_hits[0]
    ref_cy = ref["y"] + ref["h"] / 2.0
    best_idx = min(
        range(len(connect_btns)),
        key=lambda i: abs((connect_btns[i]["y"] + connect_btns[i]["h"] / 2.0) - ref_cy),
    )
    uia_invoke(hwnd, name="Connect", control_type="ControlType.Button", index=best_idx, visible_only=True, min_y=ct)


def gui_pair_demo_device(
    gui: GuiAppSession,
    model_label_contains: str,
    *,
    toggle_show_all: bool = False,
) -> None:
    """Walk the GUI AddDevice flow to pair a demo device."""
    hwnd = gui.hwnd
    gui_click_button_matching(hwnd, ("Add Device",))
    time.sleep(0.4)
    if toggle_show_all:
        gui_click_button_matching(hwnd, ("Show all models", "Show All Models"))
        time.sleep(0.3)
    gui_click_button_matching(hwnd, (model_label_contains,))
    time.sleep(0.4)
    # Enable Demo Mode toggle if not already on
    nodes = uia_get_tree(hwnd)
    demo_nodes = uia_find_nodes(nodes, name_contains="Demo Mode", visible_only=True)
    if not demo_nodes:
        raise AssertionError("Demo Mode toggle not visible on AddDevice SelectDevice stage")
    demo_node = demo_nodes[0]
    if demo_node.get("toggleState") != "On":
        uia_invoke(hwnd, name=demo_node.get("name"), control_type=demo_node.get("controlType"), visible_only=True)
        time.sleep(0.4)
    # Click the first listed demo device entry inside client area (exclude header & CSD caption buttons)
    nodes = uia_get_tree(hwnd)
    _, ct, _, _ = get_client_screen_rect(hwnd)
    excluded = {"Refresh", "Demo Mode", "Back", "Settings", "About", "Minimize", "Maximize", "Restore", "Close", ""}
    candidates = [
        n for n in nodes
        if not n.get("isOffscreen")
        and n.get("y", 0) >= ct
        and n.get("w", 0) > 40
        and n.get("h", 0) > 20
        and n.get("controlType") in {"ControlType.Button", "ControlType.ListItem"}
        and (n.get("name") or "") not in excluded
        and "settings" not in (n.get("name") or "").lower()
        and "about" not in (n.get("name") or "").lower()
    ]
    if not candidates:
        raise AssertionError(f"No demo device entry listed after enabling Demo Mode; nodes={nodes}")
    target = candidates[-1]
    click_screen_point(hwnd, target["x"] + target["w"] // 2, target["y"] + target["h"] // 2)
    time.sleep(0.6)


def gui_capture_and_audit(
    hwnd: int,
    *,
    theme: str | None = None,
    save_path: Path | None = None,
    allow_vertical_scroll: bool = True,
) -> dict[str, Any]:
    """Capture HWND client screenshot, optionally check theme luminance, and run UIA geometry audit."""
    img = capture_hwnd_client(hwnd)
    if save_path is not None:
        save_path.parent.mkdir(parents=True, exist_ok=True)
        img.save(save_path)
    if theme is not None:
        stats = verify_theme_luminance(img, theme)
    else:
        from e2e.harness.visual_capture import compute_luminance_stats
        stats = compute_luminance_stats(img)
    nodes = uia_get_tree(hwnd)
    crect = get_client_screen_rect(hwnd)
    audit = audit_uia_geometry(nodes, crect, allow_vertical_scroll=allow_vertical_scroll)
    return {
        "width": img.width,
        "height": img.height,
        "pixel_stats": stats,
        "uia_audit": audit.to_dict(),
        "nodes": nodes,
    }


def check_no_placeholder_text(nodes: list[dict[str, Any]]) -> list[str]:
    """Return any placeholder strings found in visible UIA node names/values."""
    bad_patterns = (
        re.compile(r"\bTODO\b", re.IGNORECASE),
        re.compile(r"\bFIXME\b", re.IGNORECASE),
        re.compile(r"lorem\s+ipsum", re.IGNORECASE),
        re.compile(r"\bunreachable\b", re.IGNORECASE),
    )
    hits: list[str] = []
    for n in nodes:
        for field in (n.get("name") or "", n.get("value") or ""):
            for pat in bad_patterns:
                if pat.search(field):
                    hits.append(field)
    return hits
