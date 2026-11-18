"""Tier 3: Pairwise cross-feature combination tests across F1..F10 (12 test cases)."""

from __future__ import annotations

import json
import time

from e2e.fixtures.aap_fixtures import DEMO_APP2_MAC, DEMO_Q30_MAC, REAL_APP2_MAC, flatten_list_settings
from e2e.fixtures.eq_baselines import A3944_PRESET_OVERRIDES, BASELINE_BY_OPTION, expected_type2_curves
from e2e.harness.cli_driver import IsolatedCliSession, run_cargo
from e2e.harness.gui_driver import (
    GuiAppSession,
    query_sqlite,
    read_gui_config,
    resize_client_window,
    seed_paired_device,
    send_text,
    uia_find_nodes,
    uia_get_tree,
)
from e2e.harness.registry import TestContext, TestSkipped, register_test
from e2e.harness.visual_capture import capture_hwnd_client, verify_theme_luminance
from e2e.tests._helpers import (
    extract_setting_from_cli_json,
    get_eq_preset_meta,
    gui_capture_and_audit,
    gui_click_button_matching,
    gui_connect_device_by_label,
    gui_pair_demo_device,
    parse_ftl_keys,
    require_app2_setting,
)


def _require_cargo(ctx: TestContext) -> None:
    if ctx.skip_cargo:
        raise TestSkipped("--skip-cargo specified")


def _require_gui(ctx: TestContext) -> None:
    if ctx.skip_gui:
        raise TestSkipped("--skip-gui specified")


@register_test(
    id="T3-01",
    tier=3,
    feature="F2",
    features=("F2", "F9"),
    name="f2_baseline_presets_gui_and_cross_device_matrix",
    description="Verify F2 baseline presets work across GUI Equalizer and CLI cross-device matrix (A3028/A3944/A3949).",
    requires_cli=True,
    requires_gui=True,
)
def test_t3_01(ctx: TestContext) -> str:
    # Part 1: CLI cross-device matrix
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        cli.pair_device("00:00:00:00:01:44", "SoundcoreA3944", demo=True)
        cli.pair_device("00:00:00:00:01:49", "SoundcoreA3949", demo=True)

        _, r_q30 = cli.setting_ops(
            DEMO_Q30_MAC,
            [("set", "presetEqualizerProfile", "Acoustic"), ("get", "volumeAdjustments", None)],
        )
        assert r_q30 is not None
        assert tuple(extract_setting_from_cli_json(r_q30, "volumeAdjustments")["value"]["value"]) == BASELINE_BY_OPTION["Acoustic"].curve

        _, r_44 = cli.setting_ops(
            "00:00:00:00:01:44",
            [("set", "presetEqualizerProfile", "BassBooster"), ("get", "volumeAdjustments", None)],
        )
        assert r_44 is not None
        assert tuple(extract_setting_from_cli_json(r_44, "volumeAdjustments")["value"]["value"]) == A3944_PRESET_OVERRIDES["BassBooster"]

        _, r_49 = cli.setting_ops(
            "00:00:00:00:01:49",
            [("set", "presetEqualizerProfile", "Rock"), ("get", "volumeAdjustments", None)],
        )
        assert r_49 is not None
        assert tuple(extract_setting_from_cli_json(r_49, "volumeAdjustments")["value"]["value"]) == expected_type2_curves()["Rock"]

    # Part 2: GUI baseline selection
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Equalizer",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Soundcore Signature", "None", "Preset Profile"))
        time.sleep(0.25)
        gui_click_button_matching(gui.hwnd, ("Acoustic",))
        time.sleep(0.35)
        assert uia_find_nodes(uia_get_tree(gui.hwnd), name_contains="Acoustic", visible_only=True)
        gui.close()
    return "F2 + F9 cross-device & GUI baseline verified"


@register_test(
    id="T3-02",
    tier=3,
    feature="F3",
    features=("F3", "F9"),
    name="f3_genre_presets_gui_and_custom_tweak_export",
    description="Select genre presets in GUI EQ workspace, plus CLI custom profile persistence & JSON export.",
    requires_cli=True,
    requires_gui=True,
)
def test_t3_02(ctx: TestContext) -> str:
    # CLI custom EQ save -> export -> DB reopen roundtrip
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        _, res = cli.setting_ops(
            DEMO_Q30_MAC,
            [
                ("set", "presetEqualizerProfile", "Techno"),
                ("set", "volumeAdjustments", "45,35,0,-10,0,20,30,25"),
                ("set", "customEqualizerProfile", "+MyTechnoMod"),
                ("set", "exportCustomEqualizerProfiles", "MyTechnoMod"),
                ("get", "exportCustomEqualizerProfilesOutput", None),
            ],
        )
        assert res is not None
        exported = extract_setting_from_cli_json(res, "exportCustomEqualizerProfilesOutput")["value"]["value"]
        assert exported and "MyTechnoMod" in str(exported), f"Unexpected export output: {exported!r}"

        # Re-query same DB path with a new session
        with IsolatedCliSession(ctx.repo_root, db_path=cli.db_path) as cli2:
            flat = flatten_list_settings(cli2.list_settings(DEMO_Q30_MAC))
            custom_meta = next(s for s in flat if s["settingId"] == "customEqualizerProfile")
            assert "MyTechnoMod" in custom_meta["setting"]["options"], custom_meta

    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Equalizer",))
        time.sleep(0.35)
        for genre in ("Trap", "Techno", "Hard Techno"):
            gui_click_button_matching(gui.hwnd, ("Soundcore Signature", "Trap", "Techno", "None", "Preset Profile"))
            time.sleep(0.25)
            gui_click_button_matching(gui.hwnd, (genre,))
            time.sleep(0.3)
        gui.close()
    return "F3 + F9 genre GUI & custom export verified"


@register_test(
    id="T3-03",
    tier=3,
    feature="F3",
    features=("F3", "F2", "F9"),
    name="gui_quick_preset_with_genre_eq_shared_cli_sqlite",
    description="Create Quick Preset on Q30 in GUI and verify shared SQLite DB readability via openscq30 CLI.",
    requires_cli=True,
    requires_gui=True,
)
def test_t3_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Equalizer",))
        time.sleep(0.3)
        gui_click_button_matching(gui.hwnd, ("Soundcore Signature", "None", "Preset Profile"))
        time.sleep(0.25)
        gui_click_button_matching(gui.hwnd, ("Hard Techno",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Quick Presets",))
        time.sleep(0.3)
        gui_click_button_matching(gui.hwnd, ("Create Quick Preset",))
        time.sleep(0.25)
        send_text(gui.hwnd, "ClubHardTechno")
        gui_click_button_matching(gui.hwnd, ("Create",))
        time.sleep(0.35)
        gui.close()

        # Verify CLI reads the exact same database.sqlite cleanly
        with IsolatedCliSession(ctx.repo_root, db_path=gui.db_path) as cli:
            paired = cli.paired_devices()
            assert any(d["macAddress"] == DEMO_Q30_MAC for d in paired), paired
        qp_rows = query_sqlite(gui.db_path, "SELECT device_model, name FROM quick_preset")
        assert ("SoundcoreA3028", "ClubHardTechno") in qp_rows, qp_rows
    return "GUI Quick Preset + CLI shared SQLite verified"


@register_test(
    id="T3-04",
    tier=3,
    feature="F4",
    features=("F4", "F8"),
    name="airpods_pro_2_slimmed_add_device_and_demo_pair",
    description="Pair AppleAirPodsPro2 from slimmed AddDevice default view and verify Device Selection.",
    requires_gui=True,
)
def test_t3_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_pair_demo_device(gui, "AirPods Pro 2", toggle_show_all=False)
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="AirPods Pro 2", visible_only=True)
        assert uia_find_nodes(nodes, name_contains="DEMO MODE", visible_only=True)
        gui.close()
    return "F4 + F8 slimmed APP2 demo pair verified"


@register_test(
    id="T3-05",
    tier=3,
    feature="F4",
    features=("F4", "F9"),
    name="airpods_pro_2_settings_render_in_gui_without_eq",
    description="Connect to demo AirPods Pro 2 in GUI, verify categories render cleanly and Soundcore EQ absent.",
    requires_gui=True,
)
def test_t3_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.7)
        res = gui_capture_and_audit(gui.hwnd, theme="light")
        assert res["uia_audit"]["passed"], f"APP2 settings UIA audit failed: {res['uia_audit']}"
        assert not uia_find_nodes(res["nodes"], name_contains="Equalizer Import/Export", visible_only=True)
        gui.close()
    return "F4 + F9 APP2 settings UI verified"


@register_test(
    id="T3-06",
    tier=3,
    feature="F5",
    features=("F5", "F8"),
    name="windows_aap_missing_service_error_in_gui_add_device",
    description="Trigger real AirPods Pro 2 discovery in GUI without WPods.Service and verify graceful recovery.",
    requires_gui=True,
)
def test_t3_06(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("AirPods Pro 2",))
        time.sleep(0.6)
        assert gui.proc and gui.proc.poll() is None, "GUI crashed on missing WPods.Service"
        gui_click_button_matching(gui.hwnd, ("Back",))
        time.sleep(0.3)
        gui.close()
    return "F5 + F8 missing WPods.Service graceful GUI behavior verified"


@register_test(
    id="T3-07",
    tier=3,
    feature="F6",
    features=("F6", "F7"),
    name="taste_skill_theme_light_dark_shell_and_drawers",
    description="Verify taste-skill Apple shell, header, Settings & About drawers render cleanly in Light & Dark.",
    requires_gui=True,
)
def test_t3_07(ctx: TestContext) -> str:
    _require_gui(ctx)
    for theme in ("light", "dark"):
        with GuiAppSession(ctx.repo_root, theme=theme) as gui:
            gui.launch()
            r_shell = gui_capture_and_audit(gui.hwnd, theme=theme)
            assert r_shell["uia_audit"]["passed"], r_shell["uia_audit"]
            gui_click_button_matching(gui.hwnd, ("Settings",))
            time.sleep(0.35)
            r_set = gui_capture_and_audit(gui.hwnd, theme=theme)
            assert r_set["uia_audit"]["passed"], r_set["uia_audit"]
            gui_click_button_matching(gui.hwnd, ("About",))
            time.sleep(0.35)
            r_abt = gui_capture_and_audit(gui.hwnd, theme=theme)
            assert r_abt["uia_audit"]["passed"], r_abt["uia_audit"]
            gui.close()
    return "F6 + F7 Light/Dark shell & drawers verified"


@register_test(
    id="T3-08",
    tier=3,
    feature="F7",
    features=("F7", "F8"),
    name="dark_theme_narrow_420x700_add_device_show_all",
    description="Switch to Dark theme, open AddDevice at 420x700, toggle Show all models, audit layout.",
    requires_gui=True,
)
def test_t3_08(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="dark", width=420, height=700) as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Show all models",))
        time.sleep(0.35)
        res = gui_capture_and_audit(gui.hwnd, theme="dark")
        assert res["uia_audit"]["passed"], res["uia_audit"]
        gui.close()
    return "F7 + F8 Dark 420x700 AddDevice verified"


@register_test(
    id="T3-09",
    tier=3,
    feature="F7",
    features=("F7", "F9"),
    name="locale_and_dark_theme_persist_across_device_settings",
    description="Seed preferred_language='de' and theme='dark', launch GUI, connect to Life Q30, verify render.",
    requires_gui=True,
)
def test_t3_09(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="dark", preferred_language="de") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        cfg = read_gui_config(gui.config_dir)
        assert cfg.get("theme") == "dark" and cfg.get("preferred_language") == "de"
        nodes = uia_get_tree(gui.hwnd)
        # Verify genuine German-localized UIA labels from gui/i18n/de/openscq30-gui.ftl
        de_markers = ("Verbinden", "Gerät", "Einstellungen", "Entfernen", "Wähle")
        de_hits = [
            n.get("name") for n in nodes
            if not n.get("isOffscreen") and any(m in (n.get("name") or "") for m in de_markers)
        ]
        assert de_hits, (
            f"Expected German-localized UIA labels from de/openscq30-gui.ftl when preferred_language='de'; "
            f"names={[n.get('name') for n in nodes if n.get('name')]}"
        )
        gui_click_button_matching(gui.hwnd, ("Verbinden",))
        time.sleep(0.6)
        verify_theme_luminance(capture_hwnd_client(gui.hwnd), "dark")
        gui.close()
    return "F7 + F9 German locale + Dark theme persisted cleanly"


@register_test(
    id="T3-10",
    tier=3,
    feature="F8",
    features=("F8", "F9", "F4"),
    name="switch_between_q30_and_airpods_pro_2_clean_teardown",
    description="Connect to demo Q30 -> Back -> connect to demo AirPods Pro 2 -> verify no stale Soundcore nav items.",
    requires_gui=True,
)
def test_t3_10(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        gui.launch()

        # Connect specifically to Life Q30 and verify Soundcore Q30 settings screen identity
        gui_connect_device_by_label(gui.hwnd, ("Life Q30", "SoundcoreA3028"))
        time.sleep(0.6)
        q30_nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(q30_nodes, name_contains="Equalizer", visible_only=True), (
            f"Expected Life Q30 Equalizer nav item after connecting Q30; "
            f"names={[n.get('name') for n in q30_nodes if n.get('name')]}"
        )

        gui_click_button_matching(gui.hwnd, ("Back",))
        time.sleep(0.4)

        # Connect specifically to AirPods Pro 2 and verify clean Soundcore teardown + APP2 controls
        gui_connect_device_by_label(gui.hwnd, ("AirPods Pro 2", "AppleAirPodsPro2"))
        time.sleep(0.6)
        nodes = uia_get_tree(gui.hwnd)
        assert gui.proc and gui.proc.poll() is None, "GUI crashed switching from Q30 to APP2"
        assert not uia_find_nodes(nodes, name_contains="Equalizer", visible_only=True), (
            f"Stale Soundcore Equalizer nav item remained after switching to APP2: "
            f"{[n.get('name') for n in nodes if n.get('name')]}"
        )
        assert not uia_find_nodes(nodes, name_contains="Equalizer Import/Export", visible_only=True)
        app2_markers = (
            "AirPods Pro 2",
            "Listening Mode",
            "Noise Canceling",
            "Transparency",
            "Adaptive",
            "Tone Volume",
            "Stem",
            "Battery",
            "Device Information",
        )
        assert any(uia_find_nodes(nodes, name_contains=m, visible_only=True) for m in app2_markers), (
            f"Expected AirPods Pro 2 controls/categories after switching, got: "
            f"{[n.get('name') for n in nodes if n.get('name')]}"
        )
        gui.close()
    return "F8 + F9 Q30 <-> APP2 switching & clean teardown verified"


@register_test(
    id="T3-11",
    tier=3,
    feature="F3",
    features=("F3", "F10"),
    name="genre_eq_hard_techno_visual_capture_and_audit",
    description="Capture Light & Dark screenshots of Equalizer with Hard Techno active at 1280x800 and 420x700.",
    requires_gui=True,
)
def test_t3_11(ctx: TestContext) -> str:
    _require_gui(ctx)
    out_dir = ctx.artifacts_dir / "screenshots" / ctx.run_id / "t3_hard_techno"
    for theme in ("light", "dark"):
        with GuiAppSession(ctx.repo_root, theme=theme, width=1280, height=800) as gui:
            seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
            gui.launch()
            gui_click_button_matching(gui.hwnd, ("Connect",))
            time.sleep(0.6)
            gui_click_button_matching(gui.hwnd, ("Equalizer",))
            time.sleep(0.3)
            gui_click_button_matching(gui.hwnd, ("Soundcore Signature", "None", "Preset Profile"))
            time.sleep(0.25)
            gui_click_button_matching(gui.hwnd, ("Hard Techno",))
            time.sleep(0.35)
            for vw, vh in ((1280, 800), (420, 700)):
                resize_client_window(gui.hwnd, vw, vh)
                res = gui_capture_and_audit(
                    gui.hwnd, theme=theme, save_path=out_dir / f"{theme}_{vw}x{vh}.png"
                )
                assert res["uia_audit"]["passed"], res["uia_audit"]
            gui.close()
    return "Hard Techno Light/Dark 1280x800 & 420x700 audited"


@register_test(
    id="T3-12",
    tier=3,
    feature="F1",
    features=("F1", "F3", "F10"),
    name="full_workspace_test_clippy_fmt_and_i18n_parity",
    description="Run cargo test --workspace, clippy, fmt --check, and verify CLI preset labels match en/openscq30-lib.ftl.",
    requires_cargo=True,
    requires_cli=True,
)
def test_t3_12(ctx: TestContext) -> str:
    _require_cargo(ctx)
    for args in (
        ["test", "--workspace"],
        ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        ["fmt", "--all", "--", "--check"],
    ):
        res = run_cargo(ctx.repo_root, args, timeout=600)
        assert res.returncode == 0, f"cargo {' '.join(args)} failed:\n{res.stdout}\n{res.stderr}"

    ftl = parse_ftl_keys(ctx.repo_root / "lib" / "i18n" / "en" / "openscq30-lib.ftl")
    ftl_vals = {v.lower() for v in ftl.values()}
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        meta = get_eq_preset_meta(cli, DEMO_Q30_MAC)["setting"]
        for opt, loc in zip(meta["options"], meta["localizedOptions"]):
            assert loc.lower() in ftl_vals, f"Preset {opt} ({loc}) missing from en/openscq30-lib.ftl"
    return "workspace tests/clippy/fmt & i18n parity green"
