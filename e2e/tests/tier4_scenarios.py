"""Tier 4: Real-world daily-use scenarios (5 scenarios) + 54-PNG Visual Judge Capture Pack."""

from __future__ import annotations

import json
import time
from pathlib import Path

from e2e.fixtures.aap_fixtures import DEMO_APP2_MAC, DEMO_Q30_MAC, REAL_APP2_MAC
from e2e.harness.cli_driver import IsolatedCliSession
from e2e.harness.gui_driver import (
    GuiAppSession,
    VK_CONTROL,
    VK_ESCAPE,
    VK_F,
    VK_OEM_COMMA,
    VK_TAB,
    get_client_screen_rect,
    query_sqlite,
    read_gui_config,
    resize_client_window,
    seed_paired_device,
    send_key,
    send_text,
    uia_find_nodes,
    uia_get_tree,
    uia_invoke,
    write_legacy_v1_config_toml,
)
from e2e.harness.registry import TestContext, TestSkipped, register_test
from e2e.harness.visual_capture import (
    CAPTURE_MATRIX_THEMES,
    CAPTURE_MATRIX_VIEWPORTS,
    write_visual_rubric_artifacts,
)
from e2e.tests._helpers import (
    gui_capture_and_audit,
    gui_click_button_matching,
    gui_connect_device_by_label,
    gui_pair_demo_device,
)


def _require_gui(ctx: TestContext) -> None:
    if ctx.skip_gui:
        raise TestSkipped("--skip-gui specified")


@register_test(
    id="T4-01",
    tier=4,
    feature="F8",
    features=("F6", "F7", "F8", "F4"),
    name="first_time_desktop_onboarding_q30_app2_dark_persist",
    description="Clean onboarding: pair demo Life Q30 & AirPods Pro 2 via GUI, switch to Dark theme, restart & verify.",
    requires_gui=True,
)
def test_t4_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_pair_demo_device(gui, "Life Q30")
        gui_pair_demo_device(gui, "AirPods Pro 2")
        # Switch theme to Dark
        gui_click_button_matching(gui.hwnd, ("Settings",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Light", "System Theme"))
        time.sleep(0.25)
        gui_click_button_matching(gui.hwnd, ("Dark",))
        time.sleep(0.35)
        gui.close()

        # Relaunch and verify both devices + Dark theme persist
        gui.launch()
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Life Q30", visible_only=True)
        assert uia_find_nodes(nodes, name_contains="AirPods Pro 2", visible_only=True)
        assert read_gui_config(gui.config_dir).get("theme") == "dark"
        gui.close()
    return "T4-01 onboarding & dark theme persistence passed"


@register_test(
    id="T4-02",
    tier=4,
    feature="F3",
    features=("F2", "F3", "F9"),
    name="daily_dj_listener_genre_eq_custom_export_quick_preset",
    description="Connect Q30, set Noise Canceling, switch Trap->Techno->Hard Techno, save/export custom EQ, create & activate Quick Preset.",
    requires_gui=True,
)
def test_t4_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="dark") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)

        gui_click_button_matching(gui.hwnd, ("Sound Modes",))
        time.sleep(0.3)
        gui_click_button_matching(gui.hwnd, ("Noise Canceling", "Normal", "Transparency"))
        time.sleep(0.2)
        gui_click_button_matching(gui.hwnd, ("Noise Canceling",))
        time.sleep(0.25)

        gui_click_button_matching(gui.hwnd, ("Equalizer",))
        time.sleep(0.3)
        for g in ("Trap", "Techno", "Hard Techno"):
            gui_click_button_matching(gui.hwnd, ("Soundcore Signature", "Trap", "Techno", "None", "Preset Profile"))
            time.sleep(0.2)
            gui_click_button_matching(gui.hwnd, (g,))
            time.sleep(0.25)

        # Save custom profile
        gui_click_button_matching(gui.hwnd, ("Add Custom Profile", "Add"))
        time.sleep(0.3)
        send_text(gui.hwnd, "Club Set")
        gui_click_button_matching(gui.hwnd, ("Create", "Save", "Add", "Confirm"))
        time.sleep(0.35)

        # Quick Preset
        gui_click_button_matching(gui.hwnd, ("Quick Presets",))
        time.sleep(0.3)
        gui_click_button_matching(gui.hwnd, ("Create Quick Preset",))
        time.sleep(0.25)
        send_text(gui.hwnd, "Subway Techno")
        gui_click_button_matching(gui.hwnd, ("Create",))
        time.sleep(0.3)
        gui_click_button_matching(gui.hwnd, ("Activate",))
        time.sleep(0.25)
        gui.close()

        assert ("Club Set",) in query_sqlite(gui.db_path, "SELECT name FROM equalizer_profile")
        assert ("Subway Techno",) in query_sqlite(gui.db_path, "SELECT name FROM quick_preset")
    return "T4-02 DJ genre EQ + custom profile + quick preset passed"


@register_test(
    id="T4-03",
    tier=4,
    feature="F4",
    features=("F4", "F5", "F7", "F8", "F9"),
    name="daily_airpods_pro_2_workflow_and_wpods_recovery",
    description="Attempt real APP2 without WPods.Service (graceful error), then pair/connect Demo APP2, tweak settings, remove.",
    requires_gui=True,
    requires_cli=True,
)
def test_t4_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, REAL_APP2_MAC, "AppleAirPodsPro2", demo=False)
        gui.launch()
        # Attempt connect to real APP2 without WPods.Service -> graceful warning banner, app stays alive
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(1.5)
        assert gui.proc and gui.proc.poll() is None, "GUI crashed on missing WPods.Service"
        err_nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(err_nodes, name_contains="Error:", visible_only=True), (
            f"Expected warning banner ('Error: ...') on missing WPods.Service; "
            f"names={[n.get('name') for n in err_nodes if n.get('name')]}"
        )
        _, ct, _, _ = get_client_screen_rect(gui.hwnd)
        close_btns = [
            n for n in uia_find_nodes(err_nodes, name_contains="Close", control_type="ControlType.Button", visible_only=True)
            if n.get("y", 0) >= ct
        ]
        if close_btns:
            uia_invoke(
                gui.hwnd,
                name=close_btns[0]["name"],
                control_type="ControlType.Button",
                index=0,
                visible_only=True,
                min_y=ct,
            )
            time.sleep(0.25)

        # Pair Demo APP2 via GUI & connect specifically to the Demo APP2 entry
        gui_pair_demo_device(gui, "AirPods Pro 2")
        gui_connect_device_by_label(gui.hwnd, ("DEMO MODE",))
        time.sleep(0.7)
        res = gui_capture_and_audit(gui.hwnd, theme="light")
        assert res["uia_audit"]["passed"], res["uia_audit"]
        assert not uia_find_nodes(res["nodes"], name_contains="Equalizer", visible_only=True)

        # Tweak/toggle at least one APP2 setting on Demo APP2
        _, ct, _, _ = get_client_screen_rect(gui.hwnd)
        toggles = [
            n for n in res["nodes"]
            if not n.get("isOffscreen")
            and n.get("y", 0) >= ct
            and n.get("toggleState") in ("On", "Off")
        ]
        if toggles:
            t_node = toggles[0]
            before_state = t_node["toggleState"]
            t_same = [
                n for n in uia_find_nodes(
                    res["nodes"],
                    name=t_node["name"],
                    control_type=t_node["controlType"],
                    visible_only=True,
                )
                if n.get("y", 0) >= ct
            ]
            t_idx = next((i for i, n in enumerate(t_same) if n is t_node), 0)
            uia_invoke(
                gui.hwnd,
                name=t_node["name"],
                control_type=t_node["controlType"],
                index=t_idx,
                visible_only=True,
                min_y=ct,
            )
            time.sleep(0.35)
            after_nodes = uia_get_tree(gui.hwnd)
            after_matches = uia_find_nodes(
                after_nodes,
                name=t_node["name"],
                control_type=t_node["controlType"],
                visible_only=True,
            )
            assert after_matches and after_matches[0].get("toggleState") != before_state, (
                f"APP2 toggle {t_node['name']!r} did not change state (before={before_state}, after={after_matches})"
            )
            assert not uia_find_nodes(after_nodes, name_contains="Error:", visible_only=True)
        else:
            gui_click_button_matching(gui.hwnd, ("Transparency", "Adaptive", "Noise Canceling"))
            time.sleep(0.35)
            after_nodes = uia_get_tree(gui.hwnd)
            assert not uia_find_nodes(after_nodes, name_contains="Error:", visible_only=True)

        # Navigate back and remove one paired device in UI & SQLite
        gui_click_button_matching(gui.hwnd, ("Back",))
        time.sleep(0.4)
        rm_before = uia_find_nodes(uia_get_tree(gui.hwnd), name="Remove", control_type="ControlType.Button", visible_only=True)
        assert len(rm_before) >= 2, f"Expected 2 Remove buttons, got {len(rm_before)}"
        uia_invoke(gui.hwnd, name="Remove", control_type="ControlType.Button", index=0, visible_only=True)
        time.sleep(0.3)
        rm_dlg = uia_find_nodes(uia_get_tree(gui.hwnd), name="Remove", control_type="ControlType.Button", visible_only=True)
        assert rm_dlg, "Remove confirmation button missing"
        uia_invoke(gui.hwnd, name="Remove", control_type="ControlType.Button", index=len(rm_dlg) - 1, visible_only=True)
        time.sleep(0.4)
        rm_after = uia_find_nodes(uia_get_tree(gui.hwnd), name="Remove", control_type="ControlType.Button", visible_only=True)
        assert len(rm_after) == len(rm_before) - 1, f"Expected {len(rm_before) - 1} Remove buttons after removal, got {len(rm_after)}"
        gui.close()

        # Verify shared SQLite readable via CLI and reflects removal
        with IsolatedCliSession(ctx.repo_root, db_path=gui.db_path) as cli:
            paired = cli.paired_devices()
            assert len(paired) == 1, f"Expected 1 paired device after removal, got {paired}"
    return "T4-03 AirPods Pro 2 daily workflow, tweak & removal passed"


@register_test(
    id="T4-04",
    tier=4,
    feature="F10",
    features=("F6", "F7", "F8", "F9", "F10"),
    name="keyboard_accessibility_and_420x700_compact_audit",
    description="Resize to 420x700, drive Tab/Ctrl+,/Escape/Ctrl+F, and audit UIA tree bounds & overlaps.",
    requires_gui=True,
)
def test_t4_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", width=420, height=700) as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        send_key(gui.hwnd, VK_TAB)
        send_key(gui.hwnd, VK_OEM_COMMA, ctrl=True)
        time.sleep(0.3)
        send_key(gui.hwnd, VK_ESCAPE)
        time.sleep(0.25)
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.35)
        send_key(gui.hwnd, VK_F, ctrl=True)
        time.sleep(0.25)
        send_text(gui.hwnd, "Q30")
        time.sleep(0.3)
        res = gui_capture_and_audit(gui.hwnd, theme="light")
        assert res["uia_audit"]["passed"], f"420x700 UIA audit failed: {res['uia_audit']}"
        assert uia_find_nodes(res["nodes"], name_contains="Life Q30", visible_only=True)
        gui.close()
    return "T4-04 keyboard a11y & 420x700 compact audit passed"


@register_test(
    id="T4-05",
    tier=4,
    feature="F10",
    features=("F1", "F2", "F3", "F7", "F9", "F10"),
    name="locale_legacy_migration_and_54_png_visual_judge_pack",
    description="Verify legacy v1 migration + multi-locale AND generate full 54-PNG Visual Judge Capture Pack + rubric.",
    requires_gui=True,
)
def test_t4_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    # Part A: Legacy v1 config.toml migration + Spanish locale
    with GuiAppSession(ctx.repo_root, theme="light", preferred_language="es") as gui:
        write_legacy_v1_config_toml(gui.config_dir, {"OldBass": [30, 20, 10, 0, 0, 0, 0, 0]})
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Conectar", "Connect"))
        time.sleep(0.6)
        nodes = uia_get_tree(gui.hwnd)
        mig_nodes = uia_find_nodes(nodes, name_contains="Migration", visible_only=True) or uia_find_nodes(
            nodes, name_contains="Migración", visible_only=True
        )
        assert mig_nodes, (
            f"Legacy migration nav item ('Migration'/'Migración') missing after seeding config.toml: "
            f"{[n.get('name') for n in nodes if n.get('name')]}"
        )
        gui_click_button_matching(gui.hwnd, ("Migration", "Migración"))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Migrate", "Migrar"))
        time.sleep(0.4)
        gui.close()
        migrated_rows = query_sqlite(gui.db_path, "SELECT name FROM equalizer_profile")
        assert ("OldBass",) in migrated_rows, (
            f"Legacy profile 'OldBass' was not persisted in equalizer_profile: {migrated_rows}"
        )

    # Part B: Full 54-PNG Visual Judge Capture Pack (9 screens x 3 viewports x 2 themes)
    pack_dir = ctx.artifacts_dir / "screenshots" / ctx.run_id
    entries: list[dict] = []

    for theme in CAPTURE_MATRIX_THEMES:
        for vw, vh in CAPTURE_MATRIX_VIEWPORTS:
            vp_label = f"{vw}x{vh}"

            def _capture_screen(gui: GuiAppSession, screen_id: str) -> None:
                resize_client_window(gui.hwnd, vw, vh)
                time.sleep(0.35)
                out_png = pack_dir / f"{screen_id}__{theme}__{vp_label}.png"
                res = gui_capture_and_audit(gui.hwnd, theme=theme, save_path=out_png)
                assert res["uia_audit"]["passed"], (
                    f"Visual pack UIA failure on {screen_id} ({theme} {vp_label}): {res['uia_audit']}"
                )
                entries.append(
                    {
                        "screen": screen_id,
                        "theme": theme,
                        "viewport": vp_label,
                        "file": out_png.name,
                        "pixel_stats": res["pixel_stats"],
                        "uia_audit": res["uia_audit"],
                    }
                )

            # 01 empty, 03 slimmed models, 04 select demo, 05 settings, 06 about
            with GuiAppSession(ctx.repo_root, theme=theme, width=vw, height=vh) as gui:
                gui.launch()
                _capture_screen(gui, "01_device_selection_empty")

                gui_click_button_matching(gui.hwnd, ("Settings",))
                time.sleep(0.3)
                _capture_screen(gui, "05_settings_drawer")

                gui_click_button_matching(gui.hwnd, ("About",))
                time.sleep(0.3)
                _capture_screen(gui, "06_about_drawer")
                send_key(gui.hwnd, VK_ESCAPE)
                time.sleep(0.25)

                gui_click_button_matching(gui.hwnd, ("Add Device",))
                time.sleep(0.35)
                _capture_screen(gui, "03_add_device_models_slimmed")

                gui_click_button_matching(gui.hwnd, ("Life Q30",))
                time.sleep(0.35)
                _capture_screen(gui, "04_add_device_select_demo")
                gui.close()

            # 02 populated, 07 q30 sound modes, 08 q30 eq hard techno, 09 airpods pro 2 settings
            with GuiAppSession(ctx.repo_root, theme=theme, width=vw, height=vh) as gui:
                seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
                seed_paired_device(ctx.repo_root, gui.db_path, DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
                gui.launch()
                _capture_screen(gui, "02_device_selection_populated")

                # Connect Q30 by device label and verify identity
                gui_connect_device_by_label(gui.hwnd, ("Life Q30", "SoundcoreA3028"))
                time.sleep(0.6)
                q30_nodes = uia_get_tree(gui.hwnd)
                assert uia_find_nodes(q30_nodes, name_contains="Equalizer", visible_only=True), (
                    f"Expected Life Q30 Equalizer nav before 07/08 capture; "
                    f"names={[n.get('name') for n in q30_nodes if n.get('name')]}"
                )
                gui_click_button_matching(gui.hwnd, ("Sound Modes",))
                time.sleep(0.3)
                _capture_screen(gui, "07_q30_sound_modes")

                gui_click_button_matching(gui.hwnd, ("Equalizer",))
                time.sleep(0.3)
                gui_click_button_matching(gui.hwnd, ("Soundcore Signature", "None", "Preset Profile"))
                time.sleep(0.25)
                gui_click_button_matching(gui.hwnd, ("Hard Techno",))
                time.sleep(0.3)
                _capture_screen(gui, "08_q30_equalizer_hard_techno")

                # Back -> Connect AirPods Pro 2 by device label and verify identity
                gui_click_button_matching(gui.hwnd, ("Back",))
                time.sleep(0.4)
                gui_connect_device_by_label(gui.hwnd, ("AirPods Pro 2", "AppleAirPodsPro2"))
                time.sleep(0.6)
                app2_nodes = uia_get_tree(gui.hwnd)
                assert not uia_find_nodes(app2_nodes, name_contains="Equalizer", visible_only=True), (
                    f"Expected Soundcore Equalizer absent on AirPods Pro 2 screen 09; "
                    f"names={[n.get('name') for n in app2_nodes if n.get('name')]}"
                )
                _capture_screen(gui, "09_airpods_pro_2_settings")
                gui.close()

    m_path, r_path = write_visual_rubric_artifacts(pack_dir, entries)
    manifest = json.loads(m_path.read_text(encoding="utf-8"))
    assert len(entries) == 54, f"Expected 54 PNGs in visual capture pack, got {len(entries)}"
    assert manifest["expected_total_pngs"] == 54
    assert manifest["actual_entries"] == 54, (
        f"Expected manifest actual_entries == 54, got {manifest['actual_entries']}"
    )
    assert len(manifest["entries"]) == 54
    unique_files = {entry["file"] for entry in manifest["entries"]}
    assert len(unique_files) == 54, f"Expected 54 unique PNG filenames, got {len(unique_files)}"
    for entry in manifest["entries"]:
        png_path = pack_dir / entry["file"]
        assert png_path.is_file() and png_path.stat().st_size > 0, (
            f"Visual pack PNG missing or empty on disk: {png_path}"
        )
    return f"54-PNG visual capture pack & rubric written to {pack_dir} ({r_path.name})"
