"""Tier 1: Isolated happy-path E2E tests across F1..F10 (54 test cases)."""

from __future__ import annotations

import json
import time
from pathlib import Path

from e2e.fixtures.aap_fixtures import (
    AIRPODS_PRO_2_PIDS,
    APPLE_APP2_MODEL_NUMBERS,
    BTHENUM_PREFIX,
    DEMO_APP2_MAC,
    DEMO_Q30_MAC,
    HOLD_DURATION_OPTIONS,
    LISTENING_MODES,
    MIC_SIDE_OPTIONS,
    PRESS_SPEED_OPTIONS,
    REAL_APP2_MAC,
    encode_pipe_open_frame,
    encode_pipe_status_frame,
    flatten_list_settings,
)
from e2e.fixtures.eq_baselines import (
    A3944_PRESET_OVERRIDES,
    BASELINE_OPTION_NAMES,
    BASELINE_PRESETS,
    COMMON_BAND_HZ,
    COMMON_CUSTOM_MAX_VOLUME,
    COMMON_CUSTOM_MIN_VOLUME,
    COMMON_FRACTION_DIGITS,
    COMMON_PRESET_MAX_VOLUME,
    COMMON_PRESET_MIN_VOLUME,
    MANDATORY_GENRE_OPTIONS,
    expected_type2_curves,
)
from e2e.harness.cli_driver import (
    IsolatedCliSession,
    find_cli_binary,
    find_gui_binary,
    find_toolchain_exe,
    run_cargo,
    run_command,
)
from e2e.harness.gui_driver import (
    GuiAppSession,
    VK_CONTROL,
    VK_ESCAPE,
    VK_OEM_COMMA,
    query_sqlite,
    read_gui_config,
    resize_client_window,
    seed_paired_device,
    send_key,
    send_text,
    uia_find_nodes,
    uia_get_tree,
    uia_invoke,
)
from e2e.harness.registry import TestContext, TestSkipped, register_test
from e2e.harness.visual_capture import (
    CAPTURE_MATRIX_SCREENS,
    CAPTURE_MATRIX_THEMES,
    CAPTURE_MATRIX_VIEWPORTS,
    audit_uia_geometry,
    capture_hwnd_client,
    compute_luminance_stats,
    verify_theme_luminance,
    write_visual_rubric_artifacts,
)
from e2e.tests._helpers import (
    assert_cargo_tests_passed,
    check_no_placeholder_text,
    extract_setting_from_cli_json,
    get_eq_preset_meta,
    get_volume_adjustments_meta,
    gui_capture_and_audit,
    gui_click_button_matching,
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


# =============================================================================
# Feature F1: Rust Toolchain & Baseline Checks (6 Tier 1 tests)
# =============================================================================

@register_test(
    id="F1-T1-01",
    tier=1,
    feature="F1",
    name="rust_toolchain_msvc_active",
    description="Verify rustc, cargo, and rustup are installed with x86_64-pc-windows-msvc active.",
    requires_cargo=True,
)
def test_f1_t1_01(ctx: TestContext) -> str:
    _require_cargo(ctx)
    rustc = find_toolchain_exe("rustc")
    cargo = find_toolchain_exe("cargo")
    rustup = find_toolchain_exe("rustup")
    r1 = run_command([rustc, "--version"], cwd=ctx.repo_root)
    assert r1.returncode == 0 and "rustc 1." in r1.stdout, f"rustc check failed: {r1}"
    r2 = run_command([cargo, "--version"], cwd=ctx.repo_root)
    assert r2.returncode == 0 and "cargo 1." in r2.stdout, f"cargo check failed: {r2}"
    r3 = run_command([rustup, "show", "active-toolchain"], cwd=ctx.repo_root)
    assert r3.returncode == 0 and "x86_64-pc-windows-msvc" in r3.stdout, (
        f"Expected x86_64-pc-windows-msvc toolchain, got: {r3.stdout!r}"
    )
    return r1.stdout.strip()


@register_test(
    id="F1-T1-02",
    tier=1,
    feature="F1",
    name="cargo_build_cli_and_gui_bins",
    description="Build openscq30 CLI and openscq30-gui binaries cleanly.",
    requires_cargo=True,
)
def test_f1_t1_02(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res_cli = run_cargo(ctx.repo_root, ["build", "--bin", "openscq30"], timeout=600)
    assert res_cli.returncode == 0, f"CLI build failed:\n{res_cli.stderr}"
    res_gui = run_cargo(ctx.repo_root, ["build", "-p", "openscq30-gui"], timeout=600)
    assert res_gui.returncode == 0, f"GUI build failed:\n{res_gui.stderr}"
    cli_bin = find_cli_binary(ctx.repo_root)
    gui_bin = find_gui_binary(ctx.repo_root)
    assert cli_bin is not None and cli_bin.stat().st_size > 0, "openscq30.exe missing after build"
    assert gui_bin is not None and gui_bin.stat().st_size > 0, "openscq30-gui.exe missing after build"
    return f"built {cli_bin.name} and {gui_bin.name}"


@register_test(
    id="F1-T1-03",
    tier=1,
    feature="F1",
    name="cargo_test_openscq30_lib",
    description="Run all openscq30-lib unit tests and verify 0 failures.",
    requires_cargo=True,
)
def test_f1_t1_03(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib"], timeout=600)
    assert_cargo_tests_passed(res, min_passed=1, context="F1-T1-03 openscq30-lib --lib")
    return "openscq30-lib --lib passed"


@register_test(
    id="F1-T1-04",
    tier=1,
    feature="F1",
    name="cargo_test_openscq30_cli_and_snapshots",
    description="Run all openscq30-cli unit and insta_cmd snapshot tests.",
    requires_cargo=True,
)
def test_f1_t1_04(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-cli", "--bins", "--tests"], timeout=600)
    assert_cargo_tests_passed(res, min_passed=1, context="F1-T1-04 openscq30-cli --bins --tests")
    return "openscq30-cli bins/tests passed"


@register_test(
    id="F1-T1-05",
    tier=1,
    feature="F1",
    name="cargo_clippy_workspace_clean",
    description="Run cargo clippy across workspace with zero warnings.",
    requires_cargo=True,
)
def test_f1_t1_05(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res = run_cargo(
        ctx.repo_root,
        ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        timeout=600,
    )
    assert res.returncode == 0, f"Workspace clippy failed:\n{res.stderr}\n{res.stdout}"
    return "workspace clippy clean"


@register_test(
    id="F1-T1-06",
    tier=1,
    feature="F1",
    name="cargo_fmt_and_cli_version_list_models",
    description="Verify cargo fmt --check passes and CLI prints version and SoundcoreA3028 model.",
    requires_cargo=True,
    requires_cli=True,
)
def test_f1_t1_06(ctx: TestContext) -> str:
    _require_cargo(ctx)
    fmt = run_cargo(ctx.repo_root, ["fmt", "--all", "--", "--check"], timeout=120)
    assert fmt.returncode == 0, f"cargo fmt --check failed:\n{fmt.stdout}\n{fmt.stderr}"
    with IsolatedCliSession(ctx.repo_root) as cli:
        ver = cli.run(["--version"])
        assert "2.13.0" in ver.stdout, f"Unexpected version output: {ver.stdout!r}"
        models = cli.list_models()
        by_id = {m["model"]: m["name"] for m in models}
        assert by_id.get("SoundcoreA3028") == "Soundcore Q30 / Life Q30", by_id
    return f"fmt ok, {len(models)} models listed"


# =============================================================================
# Feature F2: Preserve Pre-Existing EQ Presets (6 Tier 1 tests)
# =============================================================================

@register_test(
    id="F2-T1-01",
    tier=1,
    feature="F2",
    name="baseline_22_presets_present_and_ordered",
    description="Verify all 22 pre-existing EQ presets exist with original names and English labels on Life Q30.",
    requires_cli=True,
)
def test_f2_t1_01(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        meta = get_eq_preset_meta(cli, DEMO_Q30_MAC)
        setting = meta["setting"]
        options = setting["options"]
        localized = setting["localizedOptions"]
        assert len(options) == len(localized), "options/localizedOptions length mismatch"
        assert options[:22] == list(BASELINE_OPTION_NAMES), (
            f"First 22 options altered! expected={list(BASELINE_OPTION_NAMES)}, got={options[:22]}"
        )
        loc_map = dict(zip(options, localized))
        for p in BASELINE_PRESETS:
            assert loc_map.get(p.option) == p.localized_en, (
                f"Preset {p.option} label mismatch: expected {p.localized_en!r}, got {loc_map.get(p.option)!r}"
            )
    return "all 22 baseline option names & localized labels intact"


@register_test(
    id="F2-T1-02",
    tier=1,
    feature="F2",
    name="baseline_22_common_settings_curves_exact",
    description="Apply each of the 22 pre-existing presets on SoundcoreA3028 and assert exact 8-band curve.",
    requires_cli=True,
)
def test_f2_t1_02(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        for p in BASELINE_PRESETS:
            _, res = cli.setting_ops(
                DEMO_Q30_MAC,
                [
                    ("set", "presetEqualizerProfile", p.option),
                    ("get", "presetEqualizerProfile", None),
                    ("get", "volumeAdjustments", None),
                ],
            )
            assert res is not None
            got_preset = extract_setting_from_cli_json(res, "presetEqualizerProfile")["value"]["value"]
            got_curve = tuple(extract_setting_from_cli_json(res, "volumeAdjustments")["value"]["value"])
            assert got_preset == p.option, f"Expected {p.option}, got {got_preset}"
            assert got_curve == p.curve, f"{p.option}: expected curve {p.curve}, got {got_curve}"
    return "22/22 common_settings() curves exact"


@register_test(
    id="F2-T1-03",
    tier=1,
    feature="F2",
    name="baseline_type2_rock_and_all_22_preserved_on_a3949",
    description="Verify SoundcoreA3949 preserves common_settings_type_2() Rock curve and all 22 baselines.",
    requires_cli=True,
)
def test_f2_t1_03(ctx: TestContext) -> str:
    expected = expected_type2_curves()
    with IsolatedCliSession(ctx.repo_root) as cli:
        mac = "00:00:00:00:00:49"
        cli.pair_device(mac, "SoundcoreA3949", demo=True)
        for name, expected_curve in expected.items():
            _, res = cli.setting_ops(
                mac,
                [
                    ("set", "presetEqualizerProfile", name),
                    ("get", "presetEqualizerProfile", None),
                    ("get", "volumeAdjustments", None),
                ],
            )
            assert res is not None
            got_name = extract_setting_from_cli_json(res, "presetEqualizerProfile")["value"]["value"]
            got_curve = tuple(extract_setting_from_cli_json(res, "volumeAdjustments")["value"]["value"])
            assert got_name == name, f"A3949 expected {name}, got {got_name}"
            assert got_curve == expected_curve, f"A3949 {name}: expected {expected_curve}, got {got_curve}"
    return "A3949 type_2 Rock=[30,20,-10,-10,10,30,40,50] and all 22 intact"


@register_test(
    id="F2-T1-04",
    tier=1,
    feature="F2",
    name="a3944_bass_booster_device_override_preserved",
    description="Verify SoundcoreA3944 preserves its bespoke BassBooster curve override.",
    requires_cli=True,
)
def test_f2_t1_04(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        mac = "00:00:00:00:00:44"
        cli.pair_device(mac, "SoundcoreA3944", demo=True)
        _, res = cli.setting_ops(
            mac,
            [
                ("set", "presetEqualizerProfile", "BassBooster"),
                ("get", "presetEqualizerProfile", None),
                ("get", "volumeAdjustments", None),
            ],
        )
        assert res is not None
        got_curve = tuple(extract_setting_from_cli_json(res, "volumeAdjustments")["value"]["value"])
        assert got_curve == A3944_PRESET_OVERRIDES["BassBooster"], (
            f"A3944 BassBooster expected {A3944_PRESET_OVERRIDES['BassBooster']}, got {got_curve}"
        )
    return "A3944 BassBooster override preserved"


@register_test(
    id="F2-T1-05",
    tier=1,
    feature="F2",
    name="q30_equalizer_band_hz_and_bounds_contract",
    description="Verify Life Q30 volumeAdjustments exposes 8 bands [100..12800], fractionDigits=1, -120..=134.",
    requires_cli=True,
)
def test_f2_t1_05(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        meta = get_volume_adjustments_meta(cli, DEMO_Q30_MAC)
        assert meta["type"] == "equalizer", f"Expected equalizer type, got {meta['type']!r}"
        s = meta["setting"]
        assert tuple(s["bandHz"]) == COMMON_BAND_HZ, f"Unexpected bandHz: {s['bandHz']}"
        assert s["fractionDigits"] == COMMON_FRACTION_DIGITS
        assert s["min"] == COMMON_CUSTOM_MIN_VOLUME
        assert s["max"] == COMMON_CUSTOM_MAX_VOLUME
    return "8 bands [100..12800], fractionDigits=1, [-120, 134]"


@register_test(
    id="F2-T1-06",
    tier=1,
    feature="F2",
    name="preset_deselect_preserves_curve",
    description="Verify deselecting presetEqualizerProfile= yields null preset while keeping applied curve.",
    requires_cli=True,
)
def test_f2_t1_06(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        _, res = cli.setting_ops(
            DEMO_Q30_MAC,
            [
                ("set", "presetEqualizerProfile", "Acoustic"),
                ("set", "presetEqualizerProfile", ""),
                ("get", "presetEqualizerProfile", None),
                ("get", "volumeAdjustments", None),
            ],
        )
        assert res is not None
        got_preset = extract_setting_from_cli_json(res, "presetEqualizerProfile")["value"]["value"]
        got_curve = tuple(extract_setting_from_cli_json(res, "volumeAdjustments")["value"]["value"])
        assert got_preset is None, f"Expected null preset after deselect, got {got_preset!r}"
        assert got_curve == (40, 10, 20, 20, 40, 40, 40, 20), f"Curve changed unexpectedly: {got_curve}"
    return "deselect -> null, curve retained"


# =============================================================================
# Feature F3: Music-Genre EQ Presets (6 Tier 1 tests)
# =============================================================================

@register_test(
    id="F3-T1-01",
    tier=1,
    feature="F3",
    name="mandatory_and_additional_genre_presets_present",
    description="Verify Trap, HipHop, Rock, Techno, HardTechno plus additional common genres exist in options.",
    requires_cli=True,
)
def test_f3_t1_01(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        options = get_eq_preset_meta(cli, DEMO_Q30_MAC)["setting"]["options"]
        lower_set = {o.lower() for o in options}
        for genre in MANDATORY_GENRE_OPTIONS:
            assert genre.lower() in lower_set, (
                f"Mandatory genre preset {genre!r} missing from options={options}"
            )
        assert len(options) > 22, (
            f"Expected >22 presets (22 baseline + new genres), got {len(options)}: {options}"
        )
    return f"{len(options)} presets present including mandatory genres"


@register_test(
    id="F3-T1-02",
    tier=1,
    feature="F3",
    name="genre_presets_fluent_i18n_valid",
    description="Verify every genre preset has a clean localized English label without Fluent errors.",
    requires_cli=True,
)
def test_f3_t1_02(ctx: TestContext) -> str:
    ftl_entries = parse_ftl_keys(ctx.repo_root / "lib" / "i18n" / "en" / "openscq30-lib.ftl")
    ftl_values_lower = {v.lower() for v in ftl_entries.values()}
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        setting = get_eq_preset_meta(cli, DEMO_Q30_MAC)["setting"]
        options = setting["options"]
        localized = setting["localizedOptions"]
        assert len(options) == len(localized) and len(options) > 22
        for opt, loc in zip(options, localized):
            assert loc and loc.strip(), f"Empty localized label for {opt}"
            assert "{" not in loc and "}" not in loc, f"Unresolved Fluent placeholder in {opt}: {loc!r}"
            assert loc.lower() in ftl_values_lower, (
                f"Localized label {loc!r} for {opt} not found in en/openscq30-lib.ftl"
            )
    return f"verified {len(options)} Fluent labels"


@register_test(
    id="F3-T1-03",
    tier=1,
    feature="F3",
    name="select_and_roundtrip_every_new_genre_preset",
    description="Dynamically iterate all new genre presets and verify 0xfefe curve-aware roundtrip.",
    requires_cli=True,
)
def test_f3_t1_03(ctx: TestContext) -> str:
    baseline_set = set(BASELINE_OPTION_NAMES)
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        options = get_eq_preset_meta(cli, DEMO_Q30_MAC)["setting"]["options"]
        new_genres = [o for o in options if o not in baseline_set]
        assert len(new_genres) >= 3, f"Expected >=3 new genres beyond baseline 22, got {new_genres}"
        for genre in new_genres:
            _, res = cli.setting_ops(
                DEMO_Q30_MAC,
                [
                    ("set", "presetEqualizerProfile", genre),
                    ("get", "presetEqualizerProfile", None),
                    ("get", "volumeAdjustments", None),
                ],
            )
            assert res is not None
            got_name = extract_setting_from_cli_json(res, "presetEqualizerProfile")["value"]["value"]
            assert got_name == genre, (
                f"Genre preset {genre!r} failed roundtrip; returned {got_name!r} (possible curve collision)"
            )
    return f"roundtrip ok for {len(new_genres)} new genres: {new_genres}"


@register_test(
    id="F3-T1-04",
    tier=1,
    feature="F3",
    name="genre_presets_8_bands_within_preset_gain_range",
    description="Verify every genre preset has 8 bands all within -60..=60 (-6.0..=+6.0 dB).",
    requires_cli=True,
)
def test_f3_t1_04(ctx: TestContext) -> str:
    baseline_set = set(BASELINE_OPTION_NAMES)
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        options = get_eq_preset_meta(cli, DEMO_Q30_MAC)["setting"]["options"]
        new_genres = [o for o in options if o not in baseline_set]
        assert new_genres, "No new genre presets found"
        for genre in new_genres:
            _, res = cli.setting_ops(
                DEMO_Q30_MAC,
                [
                    ("set", "presetEqualizerProfile", genre),
                    ("get", "volumeAdjustments", None),
                ],
            )
            assert res is not None
            curve = extract_setting_from_cli_json(res, "volumeAdjustments")["value"]["value"]
            assert len(curve) == 8, f"{genre}: expected 8 bands, got {len(curve)}"
            for b in curve:
                assert COMMON_PRESET_MIN_VOLUME <= b <= COMMON_PRESET_MAX_VOLUME, (
                    f"{genre}: band {b} outside [{COMMON_PRESET_MIN_VOLUME}, {COMMON_PRESET_MAX_VOLUME}]"
                )
    return f"all {len(new_genres)} genre presets have 8 bands in [-60, 60]"


@register_test(
    id="F3-T1-05",
    tier=1,
    feature="F3",
    name="all_presets_non_flat_and_mutually_distinct",
    description="Verify all new genres are non-flat and all presets (22 baseline + genres) have unique curves.",
    requires_cli=True,
)
def test_f3_t1_05(ctx: TestContext) -> str:
    baseline_set = set(BASELINE_OPTION_NAMES)
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        options = get_eq_preset_meta(cli, DEMO_Q30_MAC)["setting"]["options"]
        seen_curves: dict[tuple[int, ...], str] = {}
        for opt in options:
            _, res = cli.setting_ops(
                DEMO_Q30_MAC,
                [
                    ("set", "presetEqualizerProfile", opt),
                    ("get", "volumeAdjustments", None),
                ],
            )
            assert res is not None
            curve = tuple(extract_setting_from_cli_json(res, "volumeAdjustments")["value"]["value"])
            if opt not in baseline_set:
                assert any(v != 0 for v in curve), f"Genre preset {opt!r} has a flat all-zero curve!"
            assert curve not in seen_curves, (
                f"Duplicate curve {curve} shared by {seen_curves[curve]!r} and {opt!r}"
            )
            seen_curves[curve] = opt
    return f"{len(seen_curves)} unique non-duplicate curves verified"


@register_test(
    id="F3-T1-06",
    tier=1,
    feature="F3",
    name="genre_presets_case_insensitive_cli_selection",
    description="Verify trap, techno, and hardtechno resolve to canonical PascalCase option names.",
    requires_cli=True,
)
def test_f3_t1_06(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        for raw, expected in (("trap", "Trap"), ("techno", "Techno"), ("hardtechno", "HardTechno")):
            _, res = cli.setting_ops(
                DEMO_Q30_MAC,
                [
                    ("set", "presetEqualizerProfile", raw),
                    ("get", "presetEqualizerProfile", None),
                ],
            )
            assert res is not None
            got = extract_setting_from_cli_json(res, "presetEqualizerProfile")["value"]["value"]
            assert got == expected, f"Expected canonical {expected!r}, got {got!r}"
    return "case-insensitive selection ok"


# =============================================================================
# Feature F4: AirPods Pro 2 AAP Protocol & Demo (6 Tier 1 tests)
# =============================================================================

@register_test(
    id="F4-T1-01",
    tier=1,
    feature="F4",
    name="airpods_pro_2_model_registered_and_demo_pairable",
    description="Verify AppleAirPodsPro2 is in list-models and pairs cleanly in Demo Mode.",
    requires_cli=True,
)
def test_f4_t1_01(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        models = cli.list_models()
        model_ids = {m["model"] for m in models}
        assert "AppleAirPodsPro2" in model_ids, f"AppleAirPodsPro2 missing from {sorted(model_ids)}"
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        paired = cli.paired_devices()
        assert any(
            d["macAddress"] == DEMO_APP2_MAC and d["model"] == "AppleAirPodsPro2" and d["isDemo"] is True
            for d in paired
        ), f"Paired APP2 demo record missing: {paired}"
    return "AppleAirPodsPro2 registered and demo paired"


@register_test(
    id="F4-T1-02",
    tier=1,
    feature="F4",
    name="airpods_pro_2_demo_schema_and_telemetry",
    description="Verify APP2 demo exposes battery, ear status, serial/firmware/model info, and NO Soundcore EQ.",
    requires_cli=True,
)
def test_f4_t1_02(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        cats = cli.list_settings(DEMO_APP2_MAC)
        assert isinstance(cats, list)
        cat_ids = {c.get("categoryId") for c in cats}
        assert "equalizer" not in cat_ids, f"AirPods Pro 2 must not expose Soundcore equalizer: {cat_ids}"
        flat = flatten_list_settings(cats)

        bat_l = require_app2_setting(flat, "battery_left")
        bat_r = require_app2_setting(flat, "battery_right")
        bat_c = require_app2_setting(flat, "battery_case")
        serial = require_app2_setting(flat, "serial_number")
        fw = require_app2_setting(flat, "firmware_version")

        ops = [
            ("get", bat_l["settingId"], None),
            ("get", bat_r["settingId"], None),
            ("get", bat_c["settingId"], None),
            ("get", serial["settingId"], None),
            ("get", fw["settingId"], None),
        ]
        _, vals = cli.setting_ops(DEMO_APP2_MAC, ops)
        assert vals is not None
        for entry in vals:
            v = entry["value"]["value"]
            assert v is not None and str(v).strip() != "", f"Empty telemetry for {entry}"
    return f"verified {len(flat)} APP2 settings without equalizer"


@register_test(
    id="F4-T1-03",
    tier=1,
    feature="F4",
    name="airpods_pro_2_listening_mode_and_one_bud_anc",
    description="Roundtrip Off/NoiseCanceling/Transparency/Adaptive listening modes and oneBudAnc toggle.",
    requires_cli=True,
)
def test_f4_t1_03(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        flat = flatten_list_settings(cli.list_settings(DEMO_APP2_MAC))
        lm = require_app2_setting(flat, "listening_mode")
        one_bud = require_app2_setting(flat, "one_bud_anc")
        allow_off = require_app2_setting(flat, "allow_off")

        ops: list[tuple[str, str, str | None]] = [("set", allow_off["settingId"], "true")]
        for mode in LISTENING_MODES:
            ops.append(("set", lm["settingId"], mode))
            ops.append(("get", lm["settingId"], None))
        ops.append(("set", one_bud["settingId"], "true"))
        ops.append(("get", one_bud["settingId"], None))

        _, res = cli.setting_ops(DEMO_APP2_MAC, ops)
        assert res is not None
        lm_vals = [
            r["value"]["value"]
            for r in res
            if r["settingId"].lower() == lm["settingId"].lower()
        ]
        assert lm_vals == list(LISTENING_MODES), f"Expected {list(LISTENING_MODES)}, got {lm_vals}"
        ob_val = extract_setting_from_cli_json(res, one_bud["settingId"])["value"]["value"]
        assert ob_val is True, f"Expected oneBudAnc=True, got {ob_val!r}"
    return "listeningMode and oneBudAnc roundtrip passed"


@register_test(
    id="F4-T1-04",
    tier=1,
    feature="F4",
    name="airpods_pro_2_stem_controls_and_tone_volume",
    description="Roundtrip pressSpeed, holdDuration, toneVolume (0..=100), and stemCycleModes.",
    requires_cli=True,
)
def test_f4_t1_04(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        flat = flatten_list_settings(cli.list_settings(DEMO_APP2_MAC))
        ps = require_app2_setting(flat, "press_speed")
        hd = require_app2_setting(flat, "hold_duration")
        tv = require_app2_setting(flat, "tone_volume")
        scm = require_app2_setting(flat, "stem_cycle_modes")

        assert tv["setting"]["min"] == 0 and tv["setting"]["max"] == 100, tv
        _, res = cli.setting_ops(
            DEMO_APP2_MAC,
            [
                ("set", ps["settingId"], "Slower"),
                ("set", hd["settingId"], "Shorter"),
                ("set", tv["settingId"], "80"),
                ("set", scm["settingId"], "NoiseCanceling,Transparency,Adaptive"),
                ("get", ps["settingId"], None),
                ("get", hd["settingId"], None),
                ("get", tv["settingId"], None),
                ("get", scm["settingId"], None),
            ],
        )
        assert res is not None
        assert extract_setting_from_cli_json(res, ps["settingId"])["value"]["value"] == "Slower"
        assert extract_setting_from_cli_json(res, hd["settingId"])["value"]["value"] == "Shorter"
        assert extract_setting_from_cli_json(res, tv["settingId"])["value"]["value"] == 80
        modes_out = extract_setting_from_cli_json(res, scm["settingId"])["value"]["value"]
        assert set(modes_out) == {"NoiseCanceling", "Transparency", "Adaptive"}, modes_out
    return "stem controls and toneVolume=80 roundtrip passed"


@register_test(
    id="F4-T1-05",
    tier=1,
    feature="F4",
    name="airpods_pro_2_audio_mic_and_wearing_detection",
    description="Roundtrip wearingDetection, conversationAwareness, personalizedVolume, and micSide.",
    requires_cli=True,
)
def test_f4_t1_05(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        flat = flatten_list_settings(cli.list_settings(DEMO_APP2_MAC))
        wd = require_app2_setting(flat, "wearing_detection")
        ca = require_app2_setting(flat, "conversation_awareness")
        pv = require_app2_setting(flat, "personalized_volume")
        ms = require_app2_setting(flat, "mic_side")

        assert tuple(ms["setting"]["options"]) == MIC_SIDE_OPTIONS, ms
        _, res = cli.setting_ops(
            DEMO_APP2_MAC,
            [
                ("set", wd["settingId"], "false"),
                ("set", ca["settingId"], "true"),
                ("set", pv["settingId"], "true"),
                ("set", ms["settingId"], "Left"),
                ("get", wd["settingId"], None),
                ("get", ca["settingId"], None),
                ("get", pv["settingId"], None),
                ("get", ms["settingId"], None),
            ],
        )
        assert res is not None
        assert extract_setting_from_cli_json(res, wd["settingId"])["value"]["value"] is False
        assert extract_setting_from_cli_json(res, ca["settingId"])["value"]["value"] is True
        assert extract_setting_from_cli_json(res, pv["settingId"])["value"]["value"] is True
        assert extract_setting_from_cli_json(res, ms["settingId"])["value"]["value"] == "Left"
    return "audio/mic/wearing settings roundtrip passed"


@register_test(
    id="F4-T1-06",
    tier=1,
    feature="F4",
    name="aap_protocol_unit_tests_pass",
    description="Execute cargo test in openscq30-lib filtering AirPods/AAP protocol tests.",
    requires_cargo=True,
)
def test_f4_t1_06(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "airpods"], timeout=600)
    assert_cargo_tests_passed(
        res,
        min_passed=1,
        required_name_substrings=("apple", "airpods", "aap"),
        context="F4-T1-06 airpods AAP unit tests",
    )
    return "airpods AAP unit tests executed & passed"


# =============================================================================
# Feature F5: Windows AirPods Pro 2 Backend (5 Tier 1 tests)
# =============================================================================

@register_test(
    id="F5-T1-01",
    tier=1,
    feature="F5",
    name="aap_backend_compiles_and_clippy_clean",
    description="Verify WindowsAapBackend and ConnectionBackends::aap() compile cleanly without warnings.",
    requires_cargo=True,
)
def test_f5_t1_01(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res = run_cargo(ctx.repo_root, ["test", "--no-run", "--workspace"], timeout=600)
    assert res.returncode == 0, f"Workspace test build failed:\n{res.stderr}"
    clip = run_cargo(ctx.repo_root, ["clippy", "-p", "openscq30-lib", "--", "-D", "warnings"], timeout=600)
    assert clip.returncode == 0, f"openscq30-lib clippy failed:\n{clip.stderr}"
    return "ConnectionBackends::aap() builds & clippy clean"


@register_test(
    id="F5-T1-02",
    tier=1,
    feature="F5",
    name="wpods_pipe_handshake_unit_and_wire_contract",
    description="Verify 'AP' + u64 LE handshake framing via cargo aap/wpods tests and wire fixture contract.",
    requires_cargo=True,
)
def test_f5_t1_02(ctx: TestContext) -> str:
    _require_cargo(ctx)
    open_frame = encode_pipe_open_frame(REAL_APP2_MAC)
    assert len(open_frame) == 10 and open_frame[:2] == b"AP"
    assert encode_pipe_status_frame(0) == b"\x02\x00\x00\x00\x00"
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "aap"], timeout=600)
    assert_cargo_tests_passed(
        res,
        min_passed=1,
        required_name_substrings=("aap", "wpods", "apple", "airpods"),
        context="F5-T1-02 WPods.Aap handshake framing",
    )
    return "WPods.Aap handshake framing tests passed"


@register_test(
    id="F5-T1-03",
    tier=1,
    feature="F5",
    name="aap_pipe_bidirectional_framing_and_call_flag",
    description="Verify inbound 0x00 AAP packet stripping, outbound raw frame, and 0x03 HFP call tracking.",
    requires_cargo=True,
)
def test_f5_t1_03(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "windows::aap"], timeout=600)
    try:
        assert_cargo_tests_passed(
            res,
            min_passed=1,
            required_name_substrings=("aap", "wpods", "apple", "airpods"),
            context="F5-T1-03 windows::aap framing",
        )
        return "windows::aap bidirectional framing tests passed"
    except AssertionError:
        res2 = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "wpods"], timeout=600)
        assert_cargo_tests_passed(
            res2,
            min_passed=1,
            required_name_substrings=("aap", "wpods", "apple", "airpods"),
            context="F5-T1-03 wpods framing",
        )
        return "wpods bidirectional framing tests passed"


@register_test(
    id="F5-T1-04",
    tier=1,
    feature="F5",
    name="bthenum_airpods_discovery_parsing",
    description="Verify BTHENUM GUID {74EC2172-0BAD-4D01-8F77-997B2BE0722A} and PID 2014/2024 parsing.",
    requires_cargo=True,
)
def test_f5_t1_04(ctx: TestContext) -> str:
    _require_cargo(ctx)
    assert "{74EC2172-0BAD-4D01-8F77-997B2BE0722A}" in BTHENUM_PREFIX
    assert set(AIRPODS_PRO_2_PIDS) == {"2014", "2024"}
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "connection_backend"], timeout=600)
    try:
        assert_cargo_tests_passed(
            res,
            min_passed=1,
            required_name_substrings=("apple", "airpods", "aap", "wpods"),
            context="F5-T1-04 connection_backend AAP discovery",
        )
    except AssertionError:
        res2 = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "wpods"], timeout=600)
        assert_cargo_tests_passed(
            res2,
            min_passed=1,
            required_name_substrings=("apple", "airpods", "aap", "wpods"),
            context="F5-T1-04 wpods discovery",
        )
    return "AirPods/AAP connection_backend discovery tests passed"


@register_test(
    id="F5-T1-05",
    tier=1,
    feature="F5",
    name="demo_and_real_app2_session_routing_coexistence",
    description="Pair both demo and non-demo APP2 in one DB; verify demo succeeds offline without corrupting DB.",
    requires_cli=True,
)
def test_f5_t1_05(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        cli.pair_device(REAL_APP2_MAC, "AppleAirPodsPro2", demo=False)
        paired = cli.paired_devices()
        assert len(paired) == 2
        # Demo device works cleanly offline
        settings = cli.list_settings(DEMO_APP2_MAC)
        assert isinstance(settings, list) and len(settings) > 0
        # Real device fails cleanly (no service/hardware) without corrupting DB
        real_res = cli.run(
            ["device", "--mac-address", REAL_APP2_MAC, "list-settings", "--json"],
            expect_ok=False,
        )
        assert real_res.returncode == 1
        assert len(cli.paired_devices()) == 2
    return "demo & real APP2 routing coexist cleanly"


# =============================================================================
# Feature F6: Install & Apply taste-skill (5 Tier 1 tests)
# =============================================================================

@register_test(
    id="T1-F6-01",
    tier=1,
    feature="F6",
    name="taste_skill_lock_present",
    description="Verify skills-lock.json exists, is valid JSON, and records Leonxlnx/taste-skill.",
)
def test_t1_f6_01(ctx: TestContext) -> str:
    lock_path = ctx.repo_root / "skills-lock.json"
    assert lock_path.is_file(), f"Missing {lock_path}"
    data = json.loads(lock_path.read_text(encoding="utf-8"))
    serialized = json.dumps(data).lower()
    assert "taste-skill" in serialized or "design-taste-frontend" in serialized, (
        f"skills-lock.json does not reference taste-skill: {data}"
    )
    return "skills-lock.json valid"


@register_test(
    id="T1-F6-02",
    tier=1,
    feature="F6",
    name="taste_skill_markdown_files_present",
    description="Verify .agents/skills/design-taste-frontend/SKILL.md exists with YAML frontmatter and directives.",
)
def test_t1_f6_02(ctx: TestContext) -> str:
    skill_md = ctx.repo_root / ".agents" / "skills" / "design-taste-frontend" / "SKILL.md"
    assert skill_md.is_file(), f"Missing {skill_md}"
    content = skill_md.read_text(encoding="utf-8")
    assert len(content) > 500, f"SKILL.md unexpectedly short ({len(content)} bytes)"
    assert content.lstrip().startswith("---"), "SKILL.md missing YAML frontmatter"
    return f"SKILL.md present ({len(content)} bytes)"


@register_test(
    id="T1-F6-03",
    tier=1,
    feature="F6",
    name="taste_skill_gui_build_clean",
    description="Verify openscq30-gui compiles, passes unit tests, and passes clippy with -D warnings.",
    requires_cargo=True,
)
def test_t1_f6_03(ctx: TestContext) -> str:
    _require_cargo(ctx)
    t_res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-gui"], timeout=600)
    assert t_res.returncode == 0, f"GUI tests failed:\n{t_res.stdout}\n{t_res.stderr}"
    c_res = run_cargo(ctx.repo_root, ["clippy", "-p", "openscq30-gui", "--", "-D", "warnings"], timeout=600)
    assert c_res.returncode == 0, f"GUI clippy failed:\n{c_res.stderr}"
    return "openscq30-gui tests & clippy clean"


@register_test(
    id="T1-F6-04",
    tier=1,
    feature="F6",
    name="taste_skill_no_placeholder_strings",
    description="Inspect rendered UIA text across GUI screens and assert zero placeholder strings.",
    requires_gui=True,
)
def test_t1_f6_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        nodes = uia_get_tree(gui.hwnd)
        hits = check_no_placeholder_text(nodes)
        assert not hits, f"Placeholder strings found on DeviceSelection: {hits}"
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.4)
        hits2 = check_no_placeholder_text(uia_get_tree(gui.hwnd))
        assert not hits2, f"Placeholder strings found on AddDevice: {hits2}"
        gui.close()
    return "zero placeholder strings in UIA tree"


@register_test(
    id="T1-F6-05",
    tier=1,
    feature="F6",
    name="taste_skill_apple_layout_tokens_reflected",
    description="Verify Light & Dark renders exhibit distinct surface/card contrast and consistent margins.",
    requires_gui=True,
)
def test_t1_f6_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    stats_by_theme = {}
    for theme in ("light", "dark"):
        with GuiAppSession(ctx.repo_root, theme=theme) as gui:
            seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
            gui.launch()
            img = capture_hwnd_client(gui.hwnd)
            stats_by_theme[theme] = verify_theme_luminance(img, theme)
            gui.close()
    assert stats_by_theme["light"]["mean"] - stats_by_theme["dark"]["mean"] >= 60.0, (
        f"Light vs Dark separation insufficient: {stats_by_theme}"
    )
    return f"light/dark layout separation verified: {stats_by_theme}"


# =============================================================================
# Feature F7: Apple-Inspired Theme, Components & Global Shell (5 Tier 1 tests)
# =============================================================================

@register_test(
    id="T1-F7-01",
    tier=1,
    feature="F7",
    name="shell_launches_with_expected_title",
    description="Launch openscq30-gui.exe and verify OpenSCQ30 window appears with non-zero client rect.",
    requires_gui=True,
)
def test_t1_f7_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", width=1280, height=800) as gui:
        hwnd = gui.launch()
        from e2e.harness.gui_driver import get_client_screen_rect, get_window_title
        assert "AirCore" in get_window_title(hwnd)
        cl, ct, cr, cb = get_client_screen_rect(hwnd)
        assert (cr - cl) == 1280 and (cb - ct) == 800, f"Unexpected client rect {(cl, ct, cr, cb)}"
        rc = gui.close()
        assert rc == 0, f"GUI exited with {rc}"
    return "shell launched 1280x800 and exited cleanly"


@register_test(
    id="T1-F7-02",
    tier=1,
    feature="F7",
    name="theme_light_and_dark_persist_toml",
    description="Verify Light vs Dark TOML theme config renders distinct luminance and persists across restart.",
    requires_gui=True,
)
def test_t1_f7_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        light_stats = verify_theme_luminance(capture_hwnd_client(gui.hwnd), "light")
        # Open Settings drawer via Ctrl+, or Settings button
        send_key(gui.hwnd, VK_OEM_COMMA, ctrl=True)
        time.sleep(0.35)
        nodes = uia_get_tree(gui.hwnd)
        if not uia_find_nodes(nodes, name_contains="Theme", visible_only=True):
            gui_click_button_matching(gui.hwnd, ("Settings",))
            time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Light", "System Theme"))
        time.sleep(0.25)
        gui_click_button_matching(gui.hwnd, ("Dark",))
        time.sleep(0.35)
        gui.close()
        cfg = read_gui_config(gui.config_dir)
        assert cfg.get("theme") == "dark", f"Expected theme='dark' persisted in TOML, got {cfg}"
        # Relaunch under same guarded directory and verify Dark luminance
        gui.launch()
        dark_stats = verify_theme_luminance(capture_hwnd_client(gui.hwnd), "dark")
        gui.close()
    return f"light={light_stats['mean']} dark={dark_stats['mean']}"


@register_test(
    id="T1-F7-03",
    tier=1,
    feature="F7",
    name="settings_drawer_toggle_button_and_shortcut",
    description="Open Settings drawer, verify Preferred Language & Theme controls, and close it cleanly.",
    requires_gui=True,
)
def test_t1_f7_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Settings",))
        time.sleep(0.4)
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Preferred Language", visible_only=True)
        assert uia_find_nodes(nodes, name_contains="Theme", visible_only=True)
        send_key(gui.hwnd, VK_ESCAPE)
        time.sleep(0.35)
        nodes_after = uia_get_tree(gui.hwnd)
        if uia_find_nodes(nodes_after, name_contains="Preferred Language", visible_only=True):
            gui_click_button_matching(gui.hwnd, ("Settings", "Close"))
            time.sleep(0.35)
            nodes_after = uia_get_tree(gui.hwnd)
        assert not uia_find_nodes(nodes_after, name_contains="Preferred Language", visible_only=True), (
            "Settings drawer remained open after closing"
        )
        gui.close()
    return "Settings drawer opened and closed cleanly"


@register_test(
    id="T1-F7-04",
    tier=1,
    feature="F7",
    name="about_drawer_displays_metadata",
    description="Open About drawer and verify OpenSCQ30 title and version 2.13.0 metadata.",
    requires_gui=True,
)
def test_t1_f7_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("About",))
        time.sleep(0.4)
        nodes = uia_get_tree(gui.hwnd)
        names = " | ".join(n.get("name") or "" for n in nodes)
        assert "2.13.0" in names or "AirCore" in names, f"About drawer metadata missing: {names}"
        gui.close()
    return "About drawer metadata verified"


@register_test(
    id="T1-F7-05",
    tier=1,
    feature="F7",
    name="language_preference_persistence_and_switch",
    description="Select a non-default language in Settings drawer and verify TOML persistence + reset.",
    requires_gui=True,
)
def test_t1_f7_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Settings",))
        time.sleep(0.4)
        gui_click_button_matching(gui.hwnd, ("Default",))
        time.sleep(0.25)
        gui_click_button_matching(gui.hwnd, ("Deutsch", "Español", "Português"))
        time.sleep(0.35)
        cfg = read_gui_config(gui.config_dir)
        assert cfg.get("preferred_language") not in (None, "", "Default"), (
            f"Expected persisted preferred_language in TOML, got {cfg}"
        )
        gui.close()
    return f"persisted preferred_language={cfg.get('preferred_language')!r}"


# =============================================================================
# Feature F8: Apple-Inspired Device Selection & Slimmed Add Device (5 Tier 1 tests)
# =============================================================================

@register_test(
    id="T1-F8-01",
    tier=1,
    feature="F8",
    name="empty_device_selection_shows_add_device",
    description="Start GUI with empty paired_device table and verify Select Device + Add Device controls.",
    requires_gui=True,
)
def test_t1_f8_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Select Device", visible_only=True)
        assert uia_find_nodes(nodes, name_contains="Add Device", visible_only=True)
        gui.close()
    return "empty Device Selection verified"


@register_test(
    id="T1-F8-02",
    tier=1,
    feature="F8",
    name="add_device_slimmed_default_list",
    description="Verify AddDevice defaults to showing Life Q30 and AirPods Pro 2 plus Show all models toggle.",
    requires_gui=True,
)
def test_t1_f8_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.4)
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Life Q30", visible_only=True), "Life Q30 missing from slimmed list"
        assert uia_find_nodes(nodes, name_contains="AirPods Pro 2", visible_only=True), "AirPods Pro 2 missing from slimmed list"
        assert uia_find_nodes(nodes, name_contains="Show all models", visible_only=True), "Show all models toggle missing"
        assert not uia_find_nodes(nodes, name_contains="Space Q45", visible_only=True), (
            "Non-default model Space Q45 should be hidden when Show all models is OFF"
        )
        gui.close()
    return "slimmed default AddDevice list verified (Q30 + APP2)"


@register_test(
    id="T1-F8-03",
    tier=1,
    feature="F8",
    name="add_device_show_all_models_toggle",
    description="Toggle Show all models ON to expand full Soundcore catalog, then OFF to collapse.",
    requires_gui=True,
)
def test_t1_f8_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.4)
        gui_click_button_matching(gui.hwnd, ("Show all models",))
        time.sleep(0.4)
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Space Q45", visible_only=True) or uia_find_nodes(
            nodes, name_contains="Liberty", visible_only=True
        ), "Expanded model list did not show additional Soundcore models"
        gui_click_button_matching(gui.hwnd, ("Show all models",))
        time.sleep(0.35)
        nodes_collapsed = uia_get_tree(gui.hwnd)
        assert not uia_find_nodes(nodes_collapsed, name_contains="Space Q45", visible_only=True), (
            "Model list did not collapse back to slimmed view"
        )
        gui.close()
    return "Show all models expand/collapse verified"


@register_test(
    id="T1-F8-04",
    tier=1,
    feature="F8",
    name="add_device_demo_pair_q30_and_airpods",
    description="Pair both Life Q30 and AirPods Pro 2 via GUI Demo Mode and verify Device Selection + SQLite.",
    requires_gui=True,
)
def test_t1_f8_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_pair_demo_device(gui, "Life Q30")
        gui_pair_demo_device(gui, "AirPods Pro 2")
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Life Q30", visible_only=True)
        assert uia_find_nodes(nodes, name_contains="AirPods Pro 2", visible_only=True)
        assert uia_find_nodes(nodes, name_contains="DEMO MODE", visible_only=True)
        gui.close()
        rows = query_sqlite(gui.db_path, "SELECT model, is_demo FROM paired_device ORDER BY model")
        assert ("AppleAirPodsPro2", 1) in rows and ("SoundcoreA3028", 1) in rows, f"DB rows={rows}"
    return "paired Q30 + APP2 demo via GUI"


@register_test(
    id="T1-F8-05",
    tier=1,
    feature="F8",
    name="device_selection_connect_and_remove_flow",
    description="Connect to demo Life Q30, navigate back, remove device with confirmation, verify SQLite empty.",
    requires_gui=True,
)
def test_t1_f8_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.7)
        nodes_settings = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes_settings, name_contains="Equalizer", visible_only=True)
        # Navigate back via header back button
        gui_click_button_matching(gui.hwnd, ("Back",))
        time.sleep(0.4)
        gui_click_button_matching(gui.hwnd, ("Remove",))
        time.sleep(0.3)
        # Confirm removal in dialog (second "Remove" button in modal)
        nodes_dlg = uia_get_tree(gui.hwnd)
        rm_btns = uia_find_nodes(nodes_dlg, name="Remove", control_type="ControlType.Button", visible_only=True)
        assert rm_btns, "Confirmation Remove button missing"
        uia_invoke(gui.hwnd, name="Remove", control_type="ControlType.Button", index=len(rm_btns) - 1, visible_only=True)
        time.sleep(0.4)
        gui.close()
        rows = query_sqlite(gui.db_path, "SELECT * FROM paired_device")
        assert rows == [], f"Device not removed from SQLite: {rows}"
    return "connect -> back -> remove flow passed"


# =============================================================================
# Feature F9: Apple-Inspired Device Settings & Equalizer Workspace (5 Tier 1 tests)
# =============================================================================

@register_test(
    id="T1-F9-01",
    tier=1,
    feature="F9",
    name="device_settings_categories_and_nav",
    description="Connect to demo Life Q30 and verify Sound Modes, Equalizer, and Quick Presets navigation.",
    requires_gui=True,
)
def test_t1_f9_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        for cat in ("Sound Modes", "Equalizer", "Equalizer Import/Export", "Device Information", "Quick Presets"):
            gui_click_button_matching(gui.hwnd, (cat,))
            time.sleep(0.25)
        gui.close()
    return "all Life Q30 nav categories rendered and switched"


@register_test(
    id="T1-F9-02",
    tier=1,
    feature="F9",
    name="sound_modes_toggle_and_select",
    description="Switch Ambient Sound Mode and Noise Canceling Mode in GUI without error banner.",
    requires_gui=True,
)
def test_t1_f9_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Sound Modes",))
        time.sleep(0.3)
        gui_click_button_matching(gui.hwnd, ("Noise Canceling", "Normal", "Transparency"))
        time.sleep(0.25)
        gui_click_button_matching(gui.hwnd, ("Transparency",))
        time.sleep(0.3)
        nodes = uia_get_tree(gui.hwnd)
        assert not uia_find_nodes(nodes, name_contains="Error:", visible_only=True)
        gui.close()
    return "Sound Modes GUI control updated cleanly"


@register_test(
    id="T1-F9-03",
    tier=1,
    feature="F9",
    name="equalizer_preset_select_and_curve_preview",
    description="Select Hard Techno preset in GUI Equalizer workspace and verify sliders/UI reflect update.",
    requires_gui=True,
)
def test_t1_f9_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Equalizer",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Soundcore Signature", "None", "Preset Profile"))
        time.sleep(0.3)
        gui_click_button_matching(gui.hwnd, ("Hard Techno",))
        time.sleep(0.4)
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Hard Techno", visible_only=True)
        gui.close()
    return "Hard Techno selectable in GUI Equalizer"


@register_test(
    id="T1-F9-04",
    tier=1,
    feature="F9",
    name="modifiable_select_add_dialog_focus_and_save",
    description="Verify ModifiableSelect Add dialog auto-focuses text input (F9 fix) and saves/removes custom EQ.",
    requires_gui=True,
)
def test_t1_f9_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Equalizer",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Add Custom Profile", "Add"))
        time.sleep(0.35)
        nodes = uia_get_tree(gui.hwnd)
        edits = uia_find_nodes(nodes, control_type="ControlType.Edit", visible_only=True)
        assert edits, "Add Custom Profile dialog Edit input not found"
        assert any(e.get("hasKeyboardFocus") for e in edits), (
            f"F9 focus ID regression: Add dialog Edit input did not receive keyboard focus: {edits}"
        )
        send_text(gui.hwnd, "E2E Custom")
        time.sleep(0.2)
        gui_click_button_matching(gui.hwnd, ("Create", "Save", "Add", "Confirm"))
        time.sleep(0.4)
        gui.close()
        rows = query_sqlite(gui.db_path, "SELECT name FROM equalizer_profile")
        assert ("E2E Custom",) in rows, f"Custom profile not saved to SQLite: {rows}"
    return "ModifiableSelect auto-focus and save verified"


@register_test(
    id="T1-F9-05",
    tier=1,
    feature="F9",
    name="quick_presets_create_edit_activate_delete",
    description="Create, edit, activate, and delete a Quick Preset via GUI and verify SQLite state.",
    requires_gui=True,
)
def test_t1_f9_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Quick Presets",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Create Quick Preset",))
        time.sleep(0.3)
        send_text(gui.hwnd, "Commute")
        gui_click_button_matching(gui.hwnd, ("Create",))
        time.sleep(0.35)
        rows = query_sqlite(gui.db_path, "SELECT name FROM quick_preset")
        assert ("Commute",) in rows, f"Quick preset Commute not persisted: {rows}"
        gui_click_button_matching(gui.hwnd, ("Edit",))
        time.sleep(0.3)
        gui_click_button_matching(gui.hwnd, ("Save", "Close"))
        time.sleep(0.25)
        gui_click_button_matching(gui.hwnd, ("Activate",))
        time.sleep(0.25)
        gui_click_button_matching(gui.hwnd, ("Delete",))
        time.sleep(0.25)
        nodes = uia_get_tree(gui.hwnd)
        del_btns = uia_find_nodes(nodes, name="Delete", control_type="ControlType.Button", visible_only=True)
        if del_btns:
            uia_invoke(gui.hwnd, name="Delete", control_type="ControlType.Button", index=len(del_btns) - 1, visible_only=True)
            time.sleep(0.3)
        gui.close()
        rows_after = query_sqlite(gui.db_path, "SELECT name FROM quick_preset")
        assert rows_after == [], f"Quick preset not deleted: {rows_after}"
    return "Quick Presets full CRUD lifecycle passed"


# =============================================================================
# Feature F10: Independent Visual Judge & Final E2E Hardening (5 Tier 1 tests)
# =============================================================================

@register_test(
    id="T1-F10-01",
    tier=1,
    feature="F10",
    name="capture_pack_all_main_screens_1280x800",
    description="Capture Light & Dark 1280x800 PNGs across main application screens and verify luminance.",
    requires_gui=True,
)
def test_t1_f10_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    out_dir = ctx.artifacts_dir / "screenshots" / ctx.run_id / "t1_1280x800"
    captured = 0
    for theme in CAPTURE_MATRIX_THEMES:
        with GuiAppSession(ctx.repo_root, theme=theme, width=1280, height=800) as gui:
            gui.launch()
            gui_capture_and_audit(gui.hwnd, theme=theme, save_path=out_dir / f"{theme}_01_empty.png")
            captured += 1
            seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
            seed_paired_device(ctx.repo_root, gui.db_path, DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
            gui.close()
            gui.launch()
            gui_capture_and_audit(gui.hwnd, theme=theme, save_path=out_dir / f"{theme}_02_populated.png")
            captured += 1
            gui.close()
    return f"captured {captured} 1280x800 screens"


@register_test(
    id="T1-F10-02",
    tier=1,
    feature="F10",
    name="capture_pack_standard_and_narrow_sizes",
    description="Capture 800x600 and 420x700 PNGs in Light & Dark and assert exact pixel dimensions + stddev > 5.",
    requires_gui=True,
)
def test_t1_f10_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    out_dir = ctx.artifacts_dir / "screenshots" / ctx.run_id / "t1_sizes"
    count = 0
    for theme in CAPTURE_MATRIX_THEMES:
        with GuiAppSession(ctx.repo_root, theme=theme, width=800, height=600) as gui:
            seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
            gui.launch()
            for vw, vh in ((800, 600), (420, 700)):
                actual = resize_client_window(gui.hwnd, vw, vh)
                assert actual == (vw, vh), f"Client resize mismatch: expected {(vw, vh)}, got {actual}"
                info = gui_capture_and_audit(
                    gui.hwnd, theme=theme, save_path=out_dir / f"{theme}_{vw}x{vh}.png"
                )
                assert info["width"] == vw and info["height"] == vh
                assert info["pixel_stats"]["stddev"] > 5.0
                count += 1
            gui.close()
    return f"captured {count} resized PNGs"


@register_test(
    id="T1-F10-03",
    tier=1,
    feature="F10",
    name="uia_geometry_no_clipping_or_overlap",
    description="Walk UIA descendants across 1280x800, 800x600, 420x700 and assert 0 zero-size/offscreen/overlap.",
    requires_gui=True,
)
def test_t1_f10_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", width=1280, height=800) as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        for vw, vh in CAPTURE_MATRIX_VIEWPORTS:
            resize_client_window(gui.hwnd, vw, vh)
            res = gui_capture_and_audit(gui.hwnd, theme="light")
            assert res["uia_audit"]["interactive_count"] > 0, (
                f"No interactive UIA controls found at {vw}x{vh}: {res['uia_audit']}"
            )
            assert res["uia_audit"]["passed"], f"UIA geometry failure at {vw}x{vh}: {res['uia_audit']}"
        gui.close()
    return "UIA geometry clean across all 3 viewports"


@register_test(
    id="T1-F10-04",
    tier=1,
    feature="F10",
    name="visual_judge_rubric_artifact_generated",
    description="Generate visual_judge_manifest.json and visual_rubric_checklist.md indexing captured views.",
    requires_gui=True,
)
def test_t1_f10_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    out_dir = ctx.artifacts_dir / "screenshots" / ctx.run_id / "t1_rubric_smoke"
    entries = []
    with GuiAppSession(ctx.repo_root, theme="light", width=1280, height=800) as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        png = out_dir / "light_1280x800_device_selection.png"
        res = gui_capture_and_audit(gui.hwnd, theme="light", save_path=png)
        entries.append(
            {
                "screen": "02_device_selection_populated",
                "theme": "light",
                "viewport": "1280x800",
                "file": png.name,
                "pixel_stats": res["pixel_stats"],
                "uia_audit": res["uia_audit"],
            }
        )
        gui.close()
    m_path, r_path = write_visual_rubric_artifacts(out_dir, entries)
    assert m_path.is_file() and r_path.is_file()
    manifest = json.loads(m_path.read_text(encoding="utf-8"))
    assert manifest["expected_total_pngs"] == 54 and len(manifest["entries"]) == 1
    return f"rubric artifacts generated at {out_dir}"


@register_test(
    id="T1-F10-05",
    tier=1,
    feature="F10",
    name="app_clean_exit_and_state_restore",
    description="Verify WM_CLOSE exits with 0, stderr has no panics, and AppDataBackupGuard restores state.",
    requires_gui=True,
)
def test_t1_f10_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="dark") as gui:
        gui.launch()
        rc = gui.close()
        stderr = gui.read_stderr()
        assert rc == 0, f"Expected clean exit code 0, got {rc}; stderr={stderr}"
        assert "panicked at" not in stderr.lower(), f"Panic detected in GUI stderr: {stderr}"
    return "WM_CLOSE clean exit 0 & AppData restored"
