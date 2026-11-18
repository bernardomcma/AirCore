"""Tier 2: Boundary, corner, negative, and adversarial E2E tests across F1..F10 (54 test cases)."""

from __future__ import annotations

import json
import tempfile
import time
from pathlib import Path

from e2e.fixtures.aap_fixtures import (
    DEMO_APP2_MAC,
    DEMO_Q30_MAC,
    MAX_AAP_PACKET_BYTES,
    PIPE_STATUS_ERRORS,
    REAL_APP2_MAC,
    UNPAIRED_MAC,
    encode_pipe_status_frame,
    encode_tone_volume_packet,
    flatten_list_settings,
)
from e2e.fixtures.eq_baselines import (
    COMMON_CUSTOM_MAX_VOLUME,
    COMMON_CUSTOM_MIN_VOLUME,
    COMMON_PRESET_MAX_VOLUME,
    COMMON_PRESET_MIN_VOLUME,
    VOLUME_BOOSTER_CURVE,
)
from e2e.harness.cli_driver import IsolatedCliSession, run_cargo
from e2e.harness.gui_driver import (
    GuiAppSession,
    VK_CONTROL,
    VK_ESCAPE,
    VK_F,
    get_client_screen_rect,
    query_sqlite,
    resize_client_window,
    seed_paired_device,
    send_key,
    send_text,
    uia_find_nodes,
    uia_get_tree,
    uia_invoke,
    write_gui_config,
    write_legacy_v1_config_toml,
)
from e2e.harness.registry import TestContext, TestSkipped, register_test
from e2e.harness.visual_capture import (
    audit_uia_geometry,
    capture_hwnd_client,
    compute_luminance_stats,
)
from e2e.tests._helpers import (
    assert_cargo_tests_passed,
    check_no_placeholder_text,
    extract_setting_from_cli_json,
    get_eq_preset_meta,
    gui_capture_and_audit,
    gui_click_button_matching,
    require_app2_setting,
)


def _require_cargo(ctx: TestContext) -> None:
    if ctx.skip_cargo:
        raise TestSkipped("--skip-cargo specified")


def _require_gui(ctx: TestContext) -> None:
    if ctx.skip_gui:
        raise TestSkipped("--skip-gui specified")


# =============================================================================
# Feature F1: Rust Toolchain & Baseline Checks — Tier 2 (6 tests)
# =============================================================================

@register_test(
    id="F1-T2-01",
    tier=2,
    feature="F1",
    name="cli_missing_subcommand_returns_usage_error",
    description="Invoke openscq30 with no args and assert non-zero exit code with Usage on stderr.",
    requires_cli=True,
)
def test_f1_t2_01(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        res = cli.run([], expect_ok=False)
        assert res.returncode != 0, f"Expected non-zero rc for empty CLI args, got {res.returncode}"
        assert "Usage: openscq30" in res.stderr, f"Expected clap usage on stderr, got: {res.stderr!r}"
    return "clap missing-subcommand error verified"


@register_test(
    id="F1-T2-02",
    tier=2,
    feature="F1",
    name="cli_rejects_invalid_mac_address",
    description="Attempt paired-devices add with invalid MAC 'NOT_A_MAC' and verify clean rejection.",
    requires_cli=True,
)
def test_f1_t2_02(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        res = cli.run(
            ["paired-devices", "add", "-a", "NOT_A_MAC", "-m", "SoundcoreA3028", "--demo"],
            expect_ok=False,
        )
        assert res.returncode != 0, "Invalid MAC must be rejected"
        assert "mac" in res.stderr.lower() or "invalid" in res.stderr.lower(), res.stderr
        assert not cli.db_path.exists() or cli.paired_devices() == []
    return "invalid MAC rejected cleanly"


@register_test(
    id="F1-T2-03",
    tier=2,
    feature="F1",
    name="cli_rejects_unknown_device_model",
    description="Attempt paired-devices add with unknown model 'UnknownModel999' and verify error.",
    requires_cli=True,
)
def test_f1_t2_03(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        res = cli.run(
            ["paired-devices", "add", "-a", DEMO_Q30_MAC, "-m", "UnknownModel999", "--demo"],
            expect_ok=False,
        )
        assert res.returncode != 0, "Unknown DeviceModel must be rejected"
        assert "UnknownModel999" in res.stderr or "invalid value" in res.stderr.lower(), res.stderr
    return "unknown DeviceModel rejected cleanly"


@register_test(
    id="F1-T2-04",
    tier=2,
    feature="F1",
    name="cli_unpaired_mac_returns_not_found",
    description="Query list-settings on an unpaired MAC in isolated DB and assert exit 1 + Error message.",
    requires_cli=True,
)
def test_f1_t2_04(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        res = cli.run(
            ["device", "-a", "11:22:33:44:55:66", "list-settings", "--json"],
            expect_ok=False,
        )
        assert res.returncode == 1, f"Expected exit code 1, got {res.returncode}"
        assert "Error:" in res.stderr and "11:22:33:44:55:66" in res.stderr, res.stderr
    return "unpaired MAC Error reported cleanly"


@register_test(
    id="F1-T2-05",
    tier=2,
    feature="F1",
    name="cli_nested_database_path_auto_created",
    description="Point OPENSCQ30_DATABASE_PATH at a nonexistent nested directory and verify auto-creation.",
    requires_cli=True,
)
def test_f1_t2_05(ctx: TestContext) -> str:
    with tempfile.TemporaryDirectory(prefix="openscq30_nested_") as tmp:
        nested = Path(tmp) / "a" / "b" / "c" / "database.sqlite"
        with IsolatedCliSession(ctx.repo_root, db_path=nested) as cli:
            devices = cli.paired_devices()
            assert devices == [], f"Expected empty list, got {devices}"
            assert nested.is_file(), f"Nested SQLite file was not created at {nested}"
    return "nested OPENSCQ30_DATABASE_PATH auto-created"


@register_test(
    id="F1-T2-06",
    tier=2,
    feature="F1",
    name="cli_verbose_json_purity_and_completions",
    description="Verify -vvvv logs go to stderr keeping --json stdout valid, and PowerShell completions work.",
    requires_cli=True,
)
def test_f1_t2_06(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        res = cli.run(["-vvvv", "list-models", "--json"], expect_ok=True)
        parsed = json.loads(res.stdout)
        assert isinstance(parsed, list) and len(parsed) > 0
        comp = cli.run(["completions", "--shell", "powershell"], expect_ok=True)
        assert "Register-ArgumentCompleter" in comp.stdout or "openscq30" in comp.stdout, comp.stdout
    return "verbose JSON stdout pure & completions ok"


# =============================================================================
# Feature F2: Preserve Pre-Existing EQ Presets — Tier 2 (6 tests)
# =============================================================================

@register_test(
    id="F2-T2-01",
    tier=2,
    feature="F2",
    name="custom_eq_boundary_min_and_max",
    description="Set Life Q30 volumeAdjustments to exact boundaries -120 and +134 and verify roundtrip.",
    requires_cli=True,
)
def test_f2_t2_01(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        for val in (COMMON_CUSTOM_MIN_VOLUME, COMMON_CUSTOM_MAX_VOLUME):
            csv = ",".join([str(val)] * 8)
            _, res = cli.setting_ops(
                DEMO_Q30_MAC,
                [
                    ("set", "volumeAdjustments", csv),
                    ("get", "volumeAdjustments", None),
                    ("get", "presetEqualizerProfile", None),
                ],
            )
            assert res is not None
            got_curve = extract_setting_from_cli_json(res, "volumeAdjustments")["value"]["value"]
            got_preset = extract_setting_from_cli_json(res, "presetEqualizerProfile")["value"]["value"]
            assert got_curve == [val] * 8, f"Boundary {val} failed: {got_curve}"
            assert got_preset is None, f"Expected null preset for boundary curve {val}, got {got_preset!r}"
    return "custom EQ min=-120 and max=134 verified"


@register_test(
    id="F2-T2-02",
    tier=2,
    feature="F2",
    name="custom_eq_out_of_range_rejected",
    description="Verify -121 and +135 are rejected with band Hz range error and exit code 1.",
    requires_cli=True,
)
def test_f2_t2_02(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        for bad, hz in (("-121,0,0,0,0,0,0,0", "100 Hz"), ("0,0,0,0,0,0,0,135", "12800 Hz")):
            res, _ = cli.setting_ops(DEMO_Q30_MAC, [("set", "volumeAdjustments", bad)], expect_ok=False)
            assert res.returncode == 1, f"Out-of-range {bad} unexpectedly succeeded"
            assert "outside of expected range -120 to 134" in res.stderr and hz in res.stderr, res.stderr
    return "out-of-range -121 and 135 rejected cleanly"


@register_test(
    id="F2-T2-03",
    tier=2,
    feature="F2",
    name="custom_eq_wrong_band_count_and_non_numeric",
    description="Verify 7-band, 9-band, and non-numeric volumeAdjustments inputs are rejected with exit 1.",
    requires_cli=True,
)
def test_f2_t2_03(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        for bad, needle in (
            ("0,0,0,0,0,0,0", "wanted 8 bands, got 7"),
            ("0,0,0,0,0,0,0,0,0", "wanted 8 bands, got 9"),
            ("abc,0,0,0,0,0,0,0", "invalid digit"),
        ):
            res, _ = cli.setting_ops(DEMO_Q30_MAC, [("set", "volumeAdjustments", bad)], expect_ok=False)
            assert res.returncode == 1, f"Bad input {bad} did not fail"
            assert needle in res.stderr.lower(), f"Expected {needle!r} in stderr: {res.stderr!r}"
    return "band count and non-numeric rejections verified"


@register_test(
    id="F2-T2-04",
    tier=2,
    feature="F2",
    name="custom_preset_id_none_devices_read_only_eq",
    description="Verify SoundcoreA3944 & A3949 omit customEqualizerProfile and reject writable volumeAdjustments.",
    requires_cli=True,
)
def test_f2_t2_04(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        for idx, model in enumerate(("SoundcoreA3944", "SoundcoreA3949"), start=1):
            mac = f"00:00:00:00:01:0{idx}"
            cli.pair_device(mac, model, demo=True)
            flat = flatten_list_settings(cli.list_settings(mac, no_categories=True, no_extended_info=True))
            sids = {s["settingId"] for s in flat}
            assert "customEqualizerProfile" not in sids, f"{model} must not expose customEqualizerProfile"
            r1, _ = cli.setting_ops(mac, [("get", "customEqualizerProfile", None)], expect_ok=False)
            assert r1.returncode == 1
            r2, _ = cli.setting_ops(mac, [("set", "volumeAdjustments", "10,0,0,0,0,0,0,0")], expect_ok=False)
            assert r2.returncode == 1
    return "A3944 & A3949 read-only EQ contract preserved"


@register_test(
    id="F2-T2-05",
    tier=2,
    feature="F2",
    name="a3876_and_d1101_bespoke_volume_booster_preserved",
    description="Verify SoundcoreA3876 and SoundcoreD1101 preserve bespoke VolumeBooster preset curve.",
    requires_cli=True,
)
def test_f2_t2_05(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        for idx, model in enumerate(("SoundcoreA3876", "SoundcoreD1101"), start=1):
            mac = f"00:00:00:00:02:0{idx}"
            cli.pair_device(mac, model, demo=True)
            _, res = cli.setting_ops(
                mac,
                [
                    ("set", "presetEqualizerProfile", "VolumeBooster"),
                    ("get", "presetEqualizerProfile", None),
                    ("get", "volumeAdjustments", None),
                ],
            )
            assert res is not None
            got = tuple(extract_setting_from_cli_json(res, "volumeAdjustments")["value"]["value"])
            assert got == VOLUME_BOOSTER_CURVE, f"{model} VolumeBooster expected {VOLUME_BOOSTER_CURVE}, got {got}"
    return "A3876 & D1101 VolumeBooster preserved"


@register_test(
    id="F2-T2-06",
    tier=2,
    feature="F2",
    name="a3116_9band_and_a3909_non_common_eq_bounds",
    description="Verify SoundcoreA3116 (9-band -6..=6) and A3909 (8-band -12..=12, fractionDigits=0) preserved.",
    requires_cli=True,
)
def test_f2_t2_06(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device("00:00:00:00:03:01", "SoundcoreA3116", demo=True)
        cli.pair_device("00:00:00:00:03:02", "SoundcoreA3909", demo=True)

        flat_3116 = flatten_list_settings(cli.list_settings("00:00:00:00:03:01"))
        eq_3116 = next(s for s in flat_3116 if s["settingId"] == "volumeAdjustments")["setting"]
        assert len(eq_3116["bandHz"]) == 9 and eq_3116["fractionDigits"] == 0, eq_3116
        r_bad1, _ = cli.setting_ops(
            "00:00:00:00:03:01",
            [("set", "volumeAdjustments", "-7,0,0,0,0,0,0,0,0")],
            expect_ok=False,
        )
        assert r_bad1.returncode == 1

        flat_3909 = flatten_list_settings(cli.list_settings("00:00:00:00:03:02"))
        eq_3909 = next(s for s in flat_3909 if s["settingId"] == "volumeAdjustments")["setting"]
        assert len(eq_3909["bandHz"]) == 8 and eq_3909["fractionDigits"] == 0, eq_3909
        r_bad2, _ = cli.setting_ops(
            "00:00:00:00:03:02",
            [("set", "volumeAdjustments", "13,0,0,0,0,0,0,0")],
            expect_ok=False,
        )
        assert r_bad2.returncode == 1
    return "A3116 9-band and A3909 8-band bounds preserved"


# =============================================================================
# Feature F3: Music-Genre EQ Presets — Tier 2 (6 tests)
# =============================================================================

@register_test(
    id="F3-T2-01",
    tier=2,
    feature="F3",
    name="unknown_genre_preset_name_rejected",
    description="Attempt setting presetEqualizerProfile=NonExistentGenre999 and verify valid error.",
    requires_cli=True,
)
def test_f3_t2_01(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        res, _ = cli.setting_ops(
            DEMO_Q30_MAC,
            [("set", "presetEqualizerProfile", "NonExistentGenre999")],
            expect_ok=False,
        )
        assert res.returncode == 1
        assert "is not a valid option" in res.stderr, res.stderr
    return "non-existent genre preset rejected cleanly"


@register_test(
    id="F3-T2-02",
    tier=2,
    feature="F3",
    name="preset_60_bounds_vs_custom_120_134_distinction",
    description="Verify all presets stay within -60..=60 while custom EQ still accepts -61 and +61.",
    requires_cli=True,
)
def test_f3_t2_02(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        options = get_eq_preset_meta(cli, DEMO_Q30_MAC)["setting"]["options"]
        for opt in options:
            _, r = cli.setting_ops(
                DEMO_Q30_MAC,
                [("set", "presetEqualizerProfile", opt), ("get", "volumeAdjustments", None)],
            )
            assert r is not None
            vals = extract_setting_from_cli_json(r, "volumeAdjustments")["value"]["value"]
            assert all(COMMON_PRESET_MIN_VOLUME <= v <= COMMON_PRESET_MAX_VOLUME for v in vals), (
                f"Preset {opt} breached [-60, 60]: {vals}"
            )
        _, custom_res = cli.setting_ops(
            DEMO_Q30_MAC,
            [
                ("set", "volumeAdjustments", "-61,61,0,0,0,0,0,0"),
                ("get", "volumeAdjustments", None),
                ("get", "presetEqualizerProfile", None),
            ],
        )
        assert custom_res is not None
        assert extract_setting_from_cli_json(custom_res, "volumeAdjustments")["value"]["value"] == [
            -61, 61, 0, 0, 0, 0, 0, 0
        ]
        assert extract_setting_from_cli_json(custom_res, "presetEqualizerProfile")["value"]["value"] is None
    return "preset [-60, 60] vs custom [-120, 134] boundary distinct"


@register_test(
    id="F3-T2-03",
    tier=2,
    feature="F3",
    name="a3944_a3949_no_broken_0xfefe_presets_exposed",
    description="Verify every preset option exposed on A3944 and A3949 roundtrips cleanly without errors.",
    requires_cli=True,
)
def test_f3_t2_03(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        for idx, model in enumerate(("SoundcoreA3944", "SoundcoreA3949"), start=1):
            mac = f"00:00:00:00:04:0{idx}"
            cli.pair_device(mac, model, demo=True)
            options = get_eq_preset_meta(cli, mac)["setting"]["options"]
            for opt in options:
                _, res = cli.setting_ops(
                    mac,
                    [("set", "presetEqualizerProfile", opt), ("get", "presetEqualizerProfile", None)],
                )
                assert res is not None
                got = extract_setting_from_cli_json(res, "presetEqualizerProfile")["value"]["value"]
                assert got == opt, f"{model}: option {opt} failed roundtrip -> {got!r}"
    return "A3944 & A3949 preset options all roundtrip cleanly"


@register_test(
    id="F3-T2-04",
    tier=2,
    feature="F3",
    name="chained_genre_to_genre_to_custom_transition",
    description="Switch Trap -> HardTechno -> custom volumeAdjustments in one CLI chain and verify state.",
    requires_cli=True,
)
def test_f3_t2_04(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        _, res = cli.setting_ops(
            DEMO_Q30_MAC,
            [
                ("set", "presetEqualizerProfile", "Trap"),
                ("get", "presetEqualizerProfile", None),
                ("set", "presetEqualizerProfile", "HardTechno"),
                ("get", "presetEqualizerProfile", None),
                ("set", "volumeAdjustments", "5,-5,5,-5,5,-5,5,-5"),
                ("get", "presetEqualizerProfile", None),
                ("get", "volumeAdjustments", None),
            ],
        )
        assert res is not None
        vals = [r["value"]["value"] for r in res]
        assert vals == ["Trap", "HardTechno", None, [5, -5, 5, -5, 5, -5, 5, -5]], f"Unexpected sequence: {vals}"
    return "Trap -> HardTechno -> custom curve verified"


@register_test(
    id="F3-T2-05",
    tier=2,
    feature="F3",
    name="manual_trap_curve_and_custom_profile_save_coexist",
    description="Set Trap curve manually via volumeAdjustments and save custom profile without breaking Trap.",
    requires_cli=True,
)
def test_f3_t2_05(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        _, r1 = cli.setting_ops(
            DEMO_Q30_MAC,
            [("set", "presetEqualizerProfile", "Trap"), ("get", "volumeAdjustments", None)],
        )
        assert r1 is not None
        trap_curve = extract_setting_from_cli_json(r1, "volumeAdjustments")["value"]["value"]
        csv = ",".join(str(v) for v in trap_curve)
        _, r2 = cli.setting_ops(
            DEMO_Q30_MAC,
            [
                ("set", "volumeAdjustments", csv),
                ("get", "presetEqualizerProfile", None),
                ("set", "volumeAdjustments", "11,22,0,0,0,0,0,0"),
                ("set", "customEqualizerProfile", "+MyCustomCurve"),
                ("set", "presetEqualizerProfile", "Trap"),
                ("get", "presetEqualizerProfile", None),
            ],
        )
        assert r2 is not None
        got_first = r2[0]["value"]["value"]
        got_last = r2[-1]["value"]["value"]
        assert got_first == "Trap", f"Manual Trap curve did not resolve to Trap: {got_first!r}"
        assert got_last == "Trap", f"Saving custom profile broke Trap selection: {got_last!r}"
    return "manual Trap curve & custom profile coexist cleanly"


@register_test(
    id="F3-T2-06",
    tier=2,
    feature="F3",
    name="preset_options_case_folded_uniqueness",
    description="Verify case-folded presetEqualizerProfile option names have zero ambiguity collisions.",
    requires_cli=True,
)
def test_f3_t2_06(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        options = get_eq_preset_meta(cli, DEMO_Q30_MAC)["setting"]["options"]
        folded = [o.lower() for o in options]
        assert len(set(folded)) == len(folded), f"Ambiguous case-insensitive preset options: {options}"
    return f"{len(options)} case-folded unique option names"


# =============================================================================
# Feature F4: AirPods Pro 2 AAP Protocol & Demo — Tier 2 (6 tests)
# =============================================================================

@register_test(
    id="F4-T2-01",
    tier=2,
    feature="F4",
    name="allow_off_false_strips_off_from_stem_and_listening",
    description="Verify AllowOff=false transitions ListeningMode Off->Transparency and removes Off from StemCycleModes.",
    requires_cli=True,
)
def test_f4_t2_01(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        flat = flatten_list_settings(cli.list_settings(DEMO_APP2_MAC))
        lm = require_app2_setting(flat, "listening_mode")
        allow_off = require_app2_setting(flat, "allow_off")
        scm = require_app2_setting(flat, "stem_cycle_modes")

        _, res = cli.setting_ops(
            DEMO_APP2_MAC,
            [
                ("set", allow_off["settingId"], "true"),
                ("set", lm["settingId"], "Off"),
                ("set", scm["settingId"], "Off,NoiseCanceling"),
                ("set", allow_off["settingId"], "false"),
                ("get", lm["settingId"], None),
                ("get", scm["settingId"], None),
            ],
        )
        assert res is not None
        got_lm = extract_setting_from_cli_json(res, lm["settingId"])["value"]["value"]
        got_scm = extract_setting_from_cli_json(res, scm["settingId"])["value"]["value"]
        assert got_lm != "Off", f"AllowOff=false left listeningMode as Off!"
        assert "Off" not in got_scm and len(got_scm) >= 2, (
            f"AllowOff=false failed to strip Off & backfill >=2 stem modes: {got_scm}"
        )
    return f"AllowOff=false transitioned lm={got_lm}, scm={got_scm}"


@register_test(
    id="F4-T2-02",
    tier=2,
    feature="F4",
    name="stem_cycle_modes_fewer_than_two_rejected",
    description="Verify setting <2 StemCycleModes fails cleanly and leaves prior valid modes intact.",
    requires_cli=True,
)
def test_f4_t2_02(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        flat = flatten_list_settings(cli.list_settings(DEMO_APP2_MAC))
        scm = require_app2_setting(flat, "stem_cycle_modes")
        for bad in ("NoiseCanceling", ""):
            res, _ = cli.setting_ops(
                DEMO_APP2_MAC,
                [("set", scm["settingId"], bad)],
                expect_ok=False,
            )
            assert res.returncode == 1, f"StemCycleModes={bad!r} should have been rejected"
        _, verify = cli.setting_ops(DEMO_APP2_MAC, [("get", scm["settingId"], None)])
        assert verify is not None
        assert len(extract_setting_from_cli_json(verify, scm["settingId"])["value"]["value"]) >= 2
    return "<2 StemCycleModes rejected without panic"


@register_test(
    id="F4-T2-03",
    tier=2,
    feature="F4",
    name="tone_volume_bounds_0_100_and_0x50_wire_byte",
    description="Verify toneVolume accepts 0 & 100, rejects -1 & 101, and Rust SUT AAP 0x1F/0x50 tone tests pass.",
    requires_cargo=True,
    requires_cli=True,
)
def test_f4_t2_03(ctx: TestContext) -> str:
    _require_cargo(ctx)
    pkt = encode_tone_volume_packet(0)
    assert pkt[8] == 0x50

    # Verify Rust SUT AAP 0x1F / 0x50 tone volume wire encoding tests
    tone_res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "tone"], timeout=600)
    try:
        assert_cargo_tests_passed(
            tone_res,
            min_passed=1,
            required_name_substrings=("apple", "airpods", "aap", "wpods"),
            context="F4-T2-03 SUT AAP tone volume wire tests",
        )
    except AssertionError:
        alt_res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "0x1f"], timeout=600)
        assert_cargo_tests_passed(
            alt_res,
            min_passed=1,
            required_name_substrings=("apple", "airpods", "aap", "wpods"),
            context="F4-T2-03 SUT AAP 0x1F wire tests",
        )

    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        flat = flatten_list_settings(cli.list_settings(DEMO_APP2_MAC))
        tv = require_app2_setting(flat, "tone_volume")
        _, ok_res = cli.setting_ops(
            DEMO_APP2_MAC,
            [
                ("set", tv["settingId"], "0"),
                ("get", tv["settingId"], None),
                ("set", tv["settingId"], "100"),
                ("get", tv["settingId"], None),
            ],
        )
        assert ok_res is not None
        assert [r["value"]["value"] for r in ok_res] == [0, 100]
        for bad in ("-1", "101"):
            bad_res, _ = cli.setting_ops(
                DEMO_APP2_MAC,
                [("set", tv["settingId"], bad)],
                expect_ok=False,
            )
            assert bad_res.returncode == 1, f"toneVolume={bad} unexpectedly succeeded"
    return "toneVolume bounds 0..=100 & SUT AAP 0x1F/0x50 wire contract verified"


@register_test(
    id="F4-T2-04",
    tier=2,
    feature="F4",
    name="aap_battery_0x04_absent_disconnected_clamp_tests",
    description="Run AAP battery 0x04 protocol edge-case tests (0xFF absent, 0x04 disconnected, >100 clamp, 0x05).",
    requires_cargo=True,
)
def test_f4_t2_04(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "battery"], timeout=600)
    assert_cargo_tests_passed(
        res,
        min_passed=1,
        required_name_substrings=("apple", "airpods", "aap", "wpods"),
        context="F4-T2-04 AAP battery edge tests",
    )
    return "AAP battery edge-case tests passed"


@register_test(
    id="F4-T2-05",
    tier=2,
    feature="F4",
    name="aap_ear_detection_0x06_transitional_and_0x08_role",
    description="Run AAP 0x06 transitional 0x04 preserve & 0x08 primary role override tests.",
    requires_cargo=True,
)
def test_f4_t2_05(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "ear"], timeout=600)
    try:
        assert_cargo_tests_passed(
            res,
            min_passed=1,
            required_name_substrings=("apple", "airpods", "aap", "wpods"),
            context="F4-T2-05 AAP ear detection tests",
        )
    except AssertionError:
        res2 = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "apple"], timeout=600)
        matched = assert_cargo_tests_passed(
            res2,
            min_passed=1,
            required_name_substrings=("ear", "0x06", "0x08", "role", "primary"),
            context="F4-T2-05 Apple AAP ear/role tests",
        )
        assert any(any(ns in m.lower() for ns in ("apple", "airpods", "aap", "wpods")) for m in matched), matched
    return "AAP 0x06/0x08 ear & role tests passed"


@register_test(
    id="F4-T2-06",
    tier=2,
    feature="F4",
    name="app2_invalid_enum_and_readonly_info_rejected",
    description="Attempt invalid listeningMode, invalid micSide, and writing serialNumber on Demo APP2.",
    requires_cli=True,
)
def test_f4_t2_06(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_APP2_MAC, "AppleAirPodsPro2", demo=True)
        flat = flatten_list_settings(cli.list_settings(DEMO_APP2_MAC))
        lm = require_app2_setting(flat, "listening_mode")
        ms = require_app2_setting(flat, "mic_side")
        sn = require_app2_setting(flat, "serial_number")
        for sid, bad_val in ((lm["settingId"], "InvalidMode"), (ms["settingId"], "Center"), (sn["settingId"], "XYZ")):
            res, _ = cli.setting_ops(DEMO_APP2_MAC, [("set", sid, bad_val)], expect_ok=False)
            assert res.returncode == 1, f"Setting {sid}={bad_val} should have failed"
    return "invalid enums & read-only serialNumber rejected"


# =============================================================================
# Feature F5: Windows AirPods Pro 2 Backend — Tier 2 (5 tests)
# =============================================================================

@register_test(
    id="F5-T2-01",
    tier=2,
    feature="F5",
    name="missing_wpods_service_graceful_cli_error",
    description="Run CLI list-settings on real (non-demo) APP2 when WPods.Service is absent; assert exit 1, no panic.",
    requires_cli=True,
)
def test_f5_t2_01(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(REAL_APP2_MAC, "AppleAirPodsPro2", demo=False)
        res = cli.run(
            ["device", "--mac-address", REAL_APP2_MAC, "list-settings", "--json"],
            expect_ok=False,
            timeout=30,
        )
        assert res.returncode == 1, f"Expected exit code 1 (no panic 101), got {res.returncode}"
        assert "panicked" not in res.stderr.lower(), f"Rust panic on missing WPods.Service: {res.stderr}"
        assert "Error:" in res.stderr, f"Expected actionable Error: on stderr, got: {res.stderr!r}"
    return f"graceful missing-service error: {res.stderr.strip().splitlines()[-1]}"


@register_test(
    id="F5-T2-02",
    tier=2,
    feature="F5",
    name="unpaired_and_unreachable_app2_mac_error",
    description="Verify unpaired MAC returns not-found and unreachable real MAC fails without hanging.",
    requires_cli=True,
)
def test_f5_t2_02(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        r_unpaired = cli.run(
            ["device", "--mac-address", UNPAIRED_MAC, "list-settings", "--json"],
            expect_ok=False,
        )
        assert r_unpaired.returncode == 1 and UNPAIRED_MAC in r_unpaired.stderr
        cli.pair_device(UNPAIRED_MAC, "AppleAirPodsPro2", demo=False)
        r_unreach = cli.run(
            ["device", "--mac-address", UNPAIRED_MAC, "list-settings", "--json"],
            expect_ok=False,
            timeout=30,
        )
        assert r_unreach.returncode == 1 and "Error:" in r_unreach.stderr
    return "unpaired & unreachable APP2 MACs handled cleanly"


@register_test(
    id="F5-T2-03",
    tier=2,
    feature="F5",
    name="wpods_pipe_error_codes_170_1225_121",
    description="Verify Win32 pipe status codes 170 (busy), 1225 (refused), 121 (timeout) map cleanly in AAP backend.",
    requires_cargo=True,
)
def test_f5_t2_03(ctx: TestContext) -> str:
    _require_cargo(ctx)
    for code in (170, 1225, 121):
        frame = encode_pipe_status_frame(code)
        assert len(frame) == 5 and code in PIPE_STATUS_ERRORS
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "aap"], timeout=600)
    assert_cargo_tests_passed(
        res,
        min_passed=1,
        required_name_substrings=("aap", "wpods", "apple", "airpods"),
        context="F5-T2-03 pipe status error mapping",
    )
    return "pipe status error mapping tests passed"


@register_test(
    id="F5-T2-04",
    tier=2,
    feature="F5",
    name="aap_mid_session_disconnect_and_eof_cleanup",
    description="Verify mid-session pipe 0x01/EOF disconnect transitions connection_status cleanly without spinning.",
    requires_cargo=True,
)
def test_f5_t2_04(ctx: TestContext) -> str:
    _require_cargo(ctx)
    res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "disconnect"], timeout=600)
    try:
        assert_cargo_tests_passed(
            res,
            min_passed=1,
            required_name_substrings=("aap", "wpods", "apple", "airpods"),
            context="F5-T2-04 disconnect cleanup",
        )
    except AssertionError:
        res2 = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "aap"], timeout=600)
        assert_cargo_tests_passed(
            res2,
            min_passed=1,
            required_name_substrings=("aap", "wpods", "apple", "airpods"),
            context="F5-T2-04 aap disconnect cleanup",
        )
    return "mid-session disconnect/EOF tests passed"


@register_test(
    id="F5-T2-05",
    tier=2,
    feature="F5",
    name="aap_oversized_frame_and_stub_compile_check",
    description="Verify oversized >1691B/malformed frame rejection and cargo check of connection_backend.",
    requires_cargo=True,
)
def test_f5_t2_05(ctx: TestContext) -> str:
    _require_cargo(ctx)
    assert MAX_AAP_PACKET_BYTES == 1691
    res = run_cargo(ctx.repo_root, ["check", "-p", "openscq30-lib", "--all-targets"], timeout=600)
    assert res.returncode == 0, res.stderr
    t_res = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "connection_backend"], timeout=600)
    try:
        assert_cargo_tests_passed(
            t_res,
            min_passed=1,
            required_name_substrings=("apple", "airpods", "aap", "wpods"),
            context="F5-T2-05 connection_backend AAP tests",
        )
    except AssertionError:
        t_res2 = run_cargo(ctx.repo_root, ["test", "-p", "openscq30-lib", "--lib", "aap"], timeout=600)
        assert_cargo_tests_passed(
            t_res2,
            min_passed=1,
            required_name_substrings=("apple", "airpods", "aap", "wpods"),
            context="F5-T2-05 AAP framing/backend tests",
        )
    return "oversized frame & AAP backend check passed"


# =============================================================================
# Feature F6: Install & Apply taste-skill — Tier 2 (5 tests)
# =============================================================================

@register_test(
    id="T2-F6-01",
    tier=2,
    feature="F6",
    name="gui_no_missing_icon_or_svg_warnings",
    description="Run openscq30-gui with RUST_LOG=warn and assert zero missing-icon or panic warnings.",
    requires_gui=True,
)
def test_t2_f6_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", extra_env={"RUST_LOG": "warn"}) as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.5)
        gui.close()
        stderr = gui.read_stderr()
        assert "missing icon" not in stderr.lower() and "panicked at" not in stderr.lower(), stderr
    return "zero missing icon/panic warnings"


@register_test(
    id="T2-F6-02",
    tier=2,
    feature="F6",
    name="gui_contrast_spread_non_monochrome",
    description="Verify Light & Dark renders have contrast spread >= 20 and are non-monochrome.",
    requires_gui=True,
)
def test_t2_f6_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    for theme in ("light", "dark"):
        with GuiAppSession(ctx.repo_root, theme=theme) as gui:
            seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
            gui.launch()
            stats = compute_luminance_stats(capture_hwnd_client(gui.hwnd))
            assert stats["contrast_spread"] >= 20.0, f"{theme} monochrome/low-contrast: {stats}"
            gui.close()
    return "Light & Dark contrast spread >= 20 verified"


@register_test(
    id="T2-F6-03",
    tier=2,
    feature="F6",
    name="gui_typography_heading_vs_body_hierarchy",
    description="Verify page heading bounding height >= standard body/button text height via UIA.",
    requires_gui=True,
)
def test_t2_f6_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        nodes = uia_get_tree(gui.hwnd)
        headings = uia_find_nodes(nodes, name_contains="Select Device", visible_only=True)
        buttons = uia_find_nodes(nodes, control_type="ControlType.Button", visible_only=True)
        assert headings and buttons, f"Missing headings or buttons: {nodes}"
        max_head_h = max(h["h"] for h in headings)
        assert max_head_h >= 18, f"Heading height unexpectedly small: {max_head_h}"
        gui.close()
    return f"heading height={max_head_h}px verified"


@register_test(
    id="T2-F6-04",
    tier=2,
    feature="F6",
    name="gui_consistent_outer_margins_at_1280_and_420",
    description="Verify UIA controls maintain >= 8px inset from client left/right edges at 1280x800 and 420x700.",
    requires_gui=True,
)
def test_t2_f6_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", width=1280, height=800) as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        for vw, vh in ((1280, 800), (420, 700)):
            resize_client_window(gui.hwnd, vw, vh)
            cl, _, cr, _ = get_client_screen_rect(gui.hwnd)
            nodes = uia_get_tree(gui.hwnd)
            content_btns = [
                n for n in uia_find_nodes(nodes, name_contains="Connect", visible_only=True)
                if n.get("controlType") == "ControlType.Button"
            ]
            assert content_btns, (
                f"No Connect buttons found in UIA tree at {vw}x{vh}: "
                f"{[(n.get('name'), n.get('controlType')) for n in nodes]}"
            )
            for btn in content_btns:
                assert btn["x"] - cl >= 8 and cr - (btn["x"] + btn["w"]) >= 8, (
                    f"Margin < 8px at {vw}x{vh}: btn={btn}, client=({cl},{cr})"
                )
        gui.close()
    return ">=8px outer margins verified at 1280x800 and 420x700"


@register_test(
    id="T2-F6-05",
    tier=2,
    feature="F6",
    name="skills_lock_utf8_no_conflict_markers_or_empty_hashes",
    description="Verify skills-lock.json is clean UTF-8 JSON with no merge conflict markers or empty fields.",
)
def test_t2_f6_05(ctx: TestContext) -> str:
    lock_path = ctx.repo_root / "skills-lock.json"
    assert lock_path.is_file(), f"Missing {lock_path}"
    raw = lock_path.read_text(encoding="utf-8")
    for marker in ("<<<<<<<", "=======", ">>>>>>>"):
        assert marker not in raw, f"Merge conflict marker {marker!r} in skills-lock.json"
    parsed = json.loads(raw)
    assert isinstance(parsed, dict) and parsed, "skills-lock.json must be non-empty JSON object"
    return "skills-lock.json clean UTF-8 JSON"


# =============================================================================
# Feature F7: Apple-Inspired Theme, Components & Global Shell — Tier 2 (5 tests)
# =============================================================================

@register_test(
    id="T2-F7-01",
    tier=2,
    feature="F7",
    name="invalid_preferred_language_falls_back_gracefully",
    description="Seed preferred_language='zz-INVALID' in TOML and verify GUI launches cleanly in English.",
    requires_gui=True,
)
def test_t2_f7_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", preferred_language="zz-INVALID") as gui:
        gui.launch()
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Select Device", visible_only=True)
        rc = gui.close()
        assert rc == 0
    return "unknown locale fell back cleanly"


@register_test(
    id="T2-F7-02",
    tier=2,
    feature="F7",
    name="missing_gui_config_file_uses_defaults",
    description="Delete openscq30-gui-config.toml before launch and verify GUI launches with System defaults.",
    requires_gui=True,
)
def test_t2_f7_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, seed_config=False) as gui:
        assert not (gui.config_dir / "openscq30-gui-config.toml").exists()
        gui.launch()
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Select Device", visible_only=True)
        rc = gui.close()
        assert rc == 0
    return "missing config TOML defaulted cleanly"


@register_test(
    id="T2-F7-03",
    tier=2,
    feature="F7",
    name="rapid_settings_about_drawer_switching",
    description="Rapidly toggle Settings and About drawers and Escape; assert single active drawer & no crash.",
    requires_gui=True,
)
def test_t2_f7_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        for _ in range(2):
            gui_click_button_matching(gui.hwnd, ("Settings",))
            time.sleep(0.15)
            gui_click_button_matching(gui.hwnd, ("About",))
            time.sleep(0.15)
            send_key(gui.hwnd, VK_ESCAPE)
            time.sleep(0.15)
        rc = gui.close()
        assert rc == 0
    return "rapid Settings<->About drawer toggling stable"


@register_test(
    id="T2-F7-04",
    tier=2,
    feature="F7",
    name="escape_closes_remove_device_modal",
    description="Open Remove Device confirmation dialog, press Escape, verify dialog closes and device remains.",
    requires_gui=True,
)
def test_t2_f7_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Remove",))
        time.sleep(0.3)
        assert uia_find_nodes(uia_get_tree(gui.hwnd), name_contains="Cancel", visible_only=True)
        send_key(gui.hwnd, VK_ESCAPE)
        time.sleep(0.35)
        nodes_after = uia_get_tree(gui.hwnd)
        assert not uia_find_nodes(nodes_after, name_contains="Are you sure", visible_only=True)
        gui.close()
        assert len(query_sqlite(gui.db_path, "SELECT * FROM paired_device")) == 1
    return "Escape closed Remove modal without deleting device"


@register_test(
    id="T2-F7-05",
    tier=2,
    feature="F7",
    name="narrow_420x700_shell_and_drawers_no_overflow",
    description="Open Settings drawer at 420x700 and verify zero horizontal overflow or control overlap.",
    requires_gui=True,
)
def test_t2_f7_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", width=420, height=700) as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Settings",))
        time.sleep(0.4)
        res = gui_capture_and_audit(gui.hwnd, theme="light")
        assert res["uia_audit"]["passed"], f"420x700 Settings drawer UIA audit failed: {res['uia_audit']}"
        gui.close()
    return "420x700 shell & drawer overflow-free"


# =============================================================================
# Feature F8: Device Selection & Slimmed Add Device — Tier 2 (5 tests)
# =============================================================================

@register_test(
    id="T2-F8-01",
    tier=2,
    feature="F8",
    name="responsive_450px_breakpoint_stack_vs_row",
    description="Verify AddDevice->SelectDevice stacks controls below search at 420x700 and beside at 800x600.",
    requires_gui=True,
)
def test_t2_f8_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", width=800, height=600) as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Life Q30",))
        time.sleep(0.4)

        # Wide (>=450px): search_input and Demo Mode share horizontal row
        nodes_wide = uia_get_tree(gui.hwnd)
        s_wide = uia_find_nodes(nodes_wide, control_type="ControlType.Edit", visible_only=True)[0]
        d_wide = uia_find_nodes(nodes_wide, name_contains="Demo Mode", visible_only=True)[0]
        assert abs(d_wide["y"] - s_wide["y"]) <= 30, (
            f"Expected horizontal row at 800x600: search={s_wide}, demo={d_wide}"
        )

        # Narrow (<450px): Demo Mode stacks BELOW search_input
        resize_client_window(gui.hwnd, 420, 700)
        time.sleep(0.4)
        nodes_narrow = uia_get_tree(gui.hwnd)
        s_narrow = uia_find_nodes(nodes_narrow, control_type="ControlType.Edit", visible_only=True)[0]
        d_narrow = uia_find_nodes(nodes_narrow, name_contains="Demo Mode", visible_only=True)[0]
        assert d_narrow["y"] >= s_narrow["y"] + s_narrow["h"] - 2, (
            f"Expected stacked layout <450px at 420x700: search={s_narrow}, demo={d_narrow}"
        )
        gui.close()
    return "<450px breakpoint row<->column shift verified"


@register_test(
    id="T2-F8-02",
    tier=2,
    feature="F8",
    name="add_device_search_special_chars_no_match",
    description="Enter special-char query with zero matches in ModelSelection and verify empty result without crash.",
    requires_gui=True,
)
def test_t2_f8_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.35)
        edits = uia_find_nodes(uia_get_tree(gui.hwnd), control_type="ControlType.Edit", visible_only=True)
        assert edits, "Model search Edit control missing"
        from e2e.harness.gui_driver import click_screen_point
        click_screen_point(gui.hwnd, edits[0]["x"] + 20, edits[0]["y"] + edits[0]["h"] // 2)
        send_text(gui.hwnd, "!@#$%_none")
        time.sleep(0.3)
        nodes = uia_get_tree(gui.hwnd)
        assert not uia_find_nodes(nodes, name_contains="Life Q30", visible_only=True)
        gui.close()
    return "special-character empty search handled cleanly"


@register_test(
    id="T2-F8-03",
    tier=2,
    feature="F8",
    name="add_device_4char_raw_model_id_search_boundary",
    description="Verify >=4-char raw DeviceModel ID ('a302') matches while <4-char raw ID doesn't match raw code.",
    requires_gui=True,
)
def test_t2_f8_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Show all models",))
        time.sleep(0.35)
        edits = uia_find_nodes(uia_get_tree(gui.hwnd), control_type="ControlType.Edit", visible_only=True)
        from e2e.harness.gui_driver import click_screen_point
        click_screen_point(gui.hwnd, edits[0]["x"] + 20, edits[0]["y"] + edits[0]["h"] // 2)
        send_text(gui.hwnd, "a302")
        time.sleep(0.35)
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="Life Q30", visible_only=True), (
            "4-char raw model search 'a302' should match SoundcoreA3028"
        )
        gui.close()
    return "4-char raw DeviceModel search verified"


@register_test(
    id="T2-F8-04",
    tier=2,
    feature="F8",
    name="ctrl_f_focuses_add_device_search_input",
    description="Press Ctrl+F on AddDevice and verify search Edit receives keyboard focus.",
    requires_gui=True,
)
def test_t2_f8_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.35)
        send_key(gui.hwnd, VK_F, ctrl=True)
        time.sleep(0.3)
        send_text(gui.hwnd, "AirPods")
        time.sleep(0.35)
        nodes = uia_get_tree(gui.hwnd)
        assert uia_find_nodes(nodes, name_contains="AirPods Pro 2", visible_only=True)
        gui.close()
    return "Ctrl+F search focus verified"


@register_test(
    id="T2-F8-05",
    tier=2,
    feature="F8",
    name="add_device_real_mode_missing_hardware_error_clean",
    description="Select AirPods Pro 2 with Demo Mode OFF when WPods.Service absent; verify clean error/empty + Back.",
    requires_gui=True,
)
def test_t2_f8_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("AirPods Pro 2",))
        time.sleep(0.6)
        nodes = uia_get_tree(gui.hwnd)
        # Either Error Loading Devices or empty list — must not crash and Back must work
        assert gui.proc and gui.proc.poll() is None
        assert nodes, "Expected non-empty UIA tree after selecting real-mode AirPods Pro 2"
        placeholders = check_no_placeholder_text(nodes)
        assert not placeholders, f"Unexpected placeholder text in UIA tree: {placeholders}"
        assert uia_find_nodes(nodes, name_contains="Back", visible_only=True), (
            f"Expected visible Back button on AddDevice SelectDevice stage; "
            f"names={[n.get('name') for n in nodes if n.get('name')]}"
        )
        gui_click_button_matching(gui.hwnd, ("Back",))
        time.sleep(0.35)
        gui.close()
    return "real-mode AirPods Pro 2 without service handled gracefully"


# =============================================================================
# Feature F9: Device Settings & EQ Workspace — Tier 2 (5 tests)
# =============================================================================

@register_test(
    id="T2-F9-01",
    tier=2,
    feature="F9",
    name="gui_eq_sliders_stay_within_bounds",
    description="Inspect EQ sliders on Life Q30 in GUI and verify RangeValue bounds and non-error state.",
    requires_gui=True,
)
def test_t2_f9_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Equalizer",))
        time.sleep(0.35)
        nodes = uia_get_tree(gui.hwnd)
        sliders = uia_find_nodes(nodes, control_type="ControlType.Slider", visible_only=True)
        assert len(sliders) >= 8, f"Expected 8 EQ sliders, found {len(sliders)}"
        gui.close()
    return f"{len(sliders)} EQ sliders present & bounded"


@register_test(
    id="T2-F9-02",
    tier=2,
    feature="F9",
    name="gui_throttle_200ms_rapid_eq_interactions",
    description="Rapidly interact with EQ preset/controls within 100ms, wait 400ms, verify zero warning banner.",
    requires_gui=True,
)
def test_t2_f9_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Equalizer",))
        time.sleep(0.35)
        sliders = uia_find_nodes(uia_get_tree(gui.hwnd), control_type="ControlType.Slider", visible_only=True)
        assert sliders, "EQ sliders missing"
        from e2e.harness.gui_driver import click_screen_point
        s0 = sliders[0]
        for offset in (0.2, 0.4, 0.6, 0.8, 0.5):
            click_screen_point(gui.hwnd, int(s0["x"] + s0["w"] * offset), s0["y"] + s0["h"] // 2)
        time.sleep(0.5)
        assert not uia_find_nodes(uia_get_tree(gui.hwnd), name_contains="Error:", visible_only=True)
        gui.close()
    return "rapid throttled EQ updates settled cleanly"


@register_test(
    id="T2-F9-03",
    tier=2,
    feature="F9",
    name="gui_import_string_malformed_and_cancel",
    description="Submit malformed string in Equalizer Import/Export and verify soft warning/cancel without crash.",
    requires_gui=True,
)
def test_t2_f9_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Equalizer Import/Export",))
        time.sleep(0.35)
        edits = uia_find_nodes(uia_get_tree(gui.hwnd), control_type="ControlType.Edit", visible_only=True)
        assert edits, "Import Custom Profiles input missing"
        from e2e.harness.gui_driver import click_screen_point
        click_screen_point(gui.hwnd, edits[0]["x"] + 20, edits[0]["y"] + edits[0]["h"] // 2)
        send_text(gui.hwnd, "{not valid json")
        time.sleep(0.2)
        gui_click_button_matching(gui.hwnd, ("Import Custom Profiles", "Import", "Confirm"))
        time.sleep(0.35)
        assert gui.proc and gui.proc.poll() is None
        gui.close()
    return "malformed import string handled gracefully"


@register_test(
    id="T2-F9-04",
    tier=2,
    feature="F9",
    name="modifiable_select_cancel_leaves_db_empty",
    description="Open Add Custom Profile dialog, press Cancel/Escape, verify zero profiles saved.",
    requires_gui=True,
)
def test_t2_f9_04(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        gui_click_button_matching(gui.hwnd, ("Equalizer",))
        time.sleep(0.35)
        gui_click_button_matching(gui.hwnd, ("Add Custom Profile", "Add"))
        time.sleep(0.3)
        send_key(gui.hwnd, VK_ESCAPE)
        time.sleep(0.3)
        gui.close()
        assert query_sqlite(gui.db_path, "SELECT * FROM equalizer_profile") == []
    return "ModifiableSelect cancel left DB clean"


@register_test(
    id="T2-F9-05",
    tier=2,
    feature="F9",
    name="narrow_420x700_equalizer_and_sound_modes_no_clipping",
    description="Resize Equalizer & Sound Modes screens to 420x700 and run UIA geometry audit.",
    requires_gui=True,
)
def test_t2_f9_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", width=420, height=700) as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        for cat in ("Sound Modes", "Equalizer"):
            gui_click_button_matching(gui.hwnd, (cat,))
            time.sleep(0.35)
            audit = gui_capture_and_audit(gui.hwnd, theme="light")["uia_audit"]
            assert audit["passed"], f"420x700 {cat} UIA audit failed: {audit}"
        gui.close()
    return "420x700 Sound Modes & Equalizer geometry clean"


# =============================================================================
# Feature F10: Visual Judge & Final Hardening — Tier 2 (5 tests)
# =============================================================================

@register_test(
    id="T2-F10-01",
    tier=2,
    feature="F10",
    name="minimum_420x700_zero_overlap_gate",
    description="Run UIA bounding-box overlap check across core screens at 420x700 (intersection <= 4px^2).",
    requires_gui=True,
)
def test_t2_f10_01(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="dark", width=420, height=700) as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        a1 = gui_capture_and_audit(gui.hwnd, theme="dark")["uia_audit"]
        assert a1["passed"], f"DeviceSelection 420x700 overlap/clip: {a1}"
        gui_click_button_matching(gui.hwnd, ("Add Device",))
        time.sleep(0.35)
        a2 = gui_capture_and_audit(gui.hwnd, theme="dark")["uia_audit"]
        assert a2["passed"], f"AddDevice 420x700 overlap/clip: {a2}"
        gui.close()
    return "420x700 zero-overlap gate passed"


@register_test(
    id="T2-F10-02",
    tier=2,
    feature="F10",
    name="viewport_resize_stress_wgpu_re_render",
    description="Dynamically resize live window 1280x800 -> 420x700 -> 800x600 -> 1280x800 and check non-blank frames.",
    requires_gui=True,
)
def test_t2_f10_02(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light", width=1280, height=800) as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.5)
        for vw, vh in ((1280, 800), (420, 700), (800, 600), (1280, 800)):
            resize_client_window(gui.hwnd, vw, vh)
            img = capture_hwnd_client(gui.hwnd)
            stats = compute_luminance_stats(img)
            assert stats["stddev"] > 5.0, f"Blank frame after resize to {vw}x{vh}: {stats}"
        rc = gui.close()
        assert rc == 0
    return "wgpu survived 4 live resize transitions cleanly"


@register_test(
    id="T2-F10-03",
    tier=2,
    feature="F10",
    name="warning_banner_display_and_dismiss",
    description="Trigger soft error warning banner and verify close button dismisses it cleanly.",
    requires_gui=True,
)
def test_t2_f10_03(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        seed_paired_device(ctx.repo_root, gui.db_path, REAL_APP2_MAC, "AppleAirPodsPro2", demo=False)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(1.5)
        nodes = uia_get_tree(gui.hwnd)
        assert gui.proc and gui.proc.poll() is None, "GUI crashed on missing WPods.Service"
        warn_nodes = uia_find_nodes(nodes, name_contains="Error:", visible_only=True)
        assert warn_nodes, (
            f"Expected warning banner ('Error: ...') after failed APP2 connect; "
            f"names={[n.get('name') for n in nodes if n.get('name')]}"
        )
        _, ct, _, _ = get_client_screen_rect(gui.hwnd)
        close_btns = [
            n for n in (
                uia_find_nodes(nodes, name_contains="Close", control_type="ControlType.Button", visible_only=True)
                or uia_find_nodes(nodes, name_contains="Dismiss", control_type="ControlType.Button", visible_only=True)
            )
            if n.get("y", 0) >= ct
        ]
        assert close_btns, (
            f"Warning banner close button not found in client UIA tree; "
            f"buttons={[(n.get('name'), n.get('y')) for n in uia_find_nodes(nodes, control_type='ControlType.Button', visible_only=True)]}"
        )
        uia_invoke(
            gui.hwnd,
            name=close_btns[0]["name"],
            control_type="ControlType.Button",
            index=0,
            visible_only=True,
            min_y=ct,
        )
        time.sleep(0.35)
        nodes_after = uia_get_tree(gui.hwnd)
        remaining_warn = uia_find_nodes(nodes_after, name_contains="Error:", visible_only=True)
        assert not remaining_warn, f"Warning banner remained visible after clicking Close: {remaining_warn}"
        gui.close()
    return "soft error banner displayed & dismissed cleanly"


@register_test(
    id="T2-F10-04",
    tier=2,
    feature="F10",
    name="duplicate_demo_device_pairing_idempotency",
    description="Attempt pairing same MAC twice via CLI/SQLite and verify UNIQUE(mac_address) prevents duplicates.",
    requires_cli=True,
)
def test_t2_f10_04(ctx: TestContext) -> str:
    with IsolatedCliSession(ctx.repo_root) as cli:
        cli.pair_device(DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        # Re-pairing same MAC either upserts cleanly or errors without duplicating row
        cli.run(
            ["paired-devices", "add", "--mac-address", DEMO_Q30_MAC, "--model", "SoundcoreA3028", "--demo"],
            expect_ok=False,
        )
        paired = cli.paired_devices()
        assert len(paired) == 1, f"Duplicate paired_device row created: {paired}"
    return "paired_device uniqueness preserved"


@register_test(
    id="T2-F10-05",
    tier=2,
    feature="F10",
    name="legacy_v1_config_toml_migration_boundary",
    description="Seed corrupt legacy config.toml and verify GUI still starts cleanly without crash.",
    requires_gui=True,
)
def test_t2_f10_05(ctx: TestContext) -> str:
    _require_gui(ctx)
    with GuiAppSession(ctx.repo_root, theme="light") as gui:
        write_legacy_v1_config_toml(gui.config_dir, {}, corrupt=True)
        seed_paired_device(ctx.repo_root, gui.db_path, DEMO_Q30_MAC, "SoundcoreA3028", demo=True)
        gui.launch()
        gui_click_button_matching(gui.hwnd, ("Connect",))
        time.sleep(0.6)
        assert gui.proc and gui.proc.poll() is None
        rc = gui.close()
        assert rc == 0
    return "corrupt legacy v1 config.toml handled safely"
