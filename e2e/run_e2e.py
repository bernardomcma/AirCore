"""Opaque-box 4-tier E2E test runner for OpenSCQ30.

Usage examples:
    python e2e/run_e2e.py --list-tests
    python e2e/run_e2e.py --dry-run --tier all
    python e2e/run_e2e.py --self-check
    python e2e/run_e2e.py --tier all --json-report e2e/artifacts/report.json
"""

from __future__ import annotations

import argparse
import ast
import json
import py_compile
import sqlite3
import sys
import tempfile
import time
import traceback
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# Ensure repo root is importable when invoked as `python e2e/run_e2e.py`
REPO_ROOT = Path(__file__).resolve().parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from PIL import Image, ImageDraw  # noqa: E402

import e2e.tests  # noqa: E402, F401  (registers all 125 tests)
from e2e.fixtures.aap_fixtures import validate_aap_fixtures  # noqa: E402
from e2e.fixtures.eq_baselines import validate_eq_fixtures  # noqa: E402
from e2e.harness.registry import (  # noqa: E402
    TestCase,
    TestContext,
    TestSkipped,
    all_tests,
    filter_tests,
    validate_registry_thresholds,
)
from e2e.harness.visual_capture import (  # noqa: E402
    audit_uia_geometry,
    verify_theme_luminance,
)


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="OpenSCQ30 4-Tier Opaque-Box E2E Runner")
    parser.add_argument(
        "--tier",
        default="all",
        choices=("1", "2", "3", "4", "all"),
        help="Test tier to select (1, 2, 3, 4, or all)",
    )
    parser.add_argument(
        "--feature",
        default=None,
        help="Filter tests by feature ID (F1..F10)",
    )
    parser.add_argument(
        "--id",
        dest="test_id",
        default=None,
        help="Filter to a single exact test ID",
    )
    parser.add_argument(
        "--list-tests",
        action="store_true",
        help="List selected tests grouped by tier and feature and exit 0",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate registry, fixtures, and execution plan without launching Rust/GUI binaries",
    )
    parser.add_argument(
        "--self-check",
        action="store_true",
        help="Run structural, AST integrity, fixture, SQLite, and geometry auditor self-checks",
    )
    parser.add_argument(
        "--json-report",
        default=None,
        help="Write machine-readable JSON execution report to this path",
    )
    parser.add_argument(
        "--skip-cargo",
        action="store_true",
        help="Explicitly skip tests that require cargo/rustc execution",
    )
    parser.add_argument(
        "--skip-gui",
        action="store_true",
        help="Explicitly skip tests that require openscq30-gui.exe",
    )
    parser.add_argument(
        "--artifacts-dir",
        default=str(REPO_ROOT / "e2e" / "artifacts"),
        help="Directory for screenshots, manifests, and reports",
    )
    return parser.parse_args(argv)


def _select_tests(args: argparse.Namespace) -> list[TestCase]:
    tier_val = None if args.tier == "all" else int(args.tier)
    if args.feature is not None:
        norm = args.feature.upper()
        if norm not in {f"F{i}" for i in range(1, 11)}:
            raise SystemExit(f"Invalid --feature {args.feature!r}; expected F1..F10")
    return filter_tests(tier=tier_val, feature=args.feature, test_id=args.test_id)


def _cmd_list_tests(tests: list[TestCase]) -> int:
    by_tier: dict[int, list[TestCase]] = {1: [], 2: [], 3: [], 4: []}
    for t in tests:
        by_tier[t.tier].append(t)
    for tier in (1, 2, 3, 4):
        group = by_tier[tier]
        if not group:
            continue
        print(f"=== Tier {tier} ({len(group)} tests) ===")
        for t in group:
            feats = ",".join(t.features)
            print(f"  {t.id:10s} [{feats:16s}] {t.name} - {t.description}")
    print(f"Total listed: {len(tests)}")
    return 0


def _ast_verify_test_modules(e2e_dir: Path) -> list[str]:
    """Verify via AST that no test function is stubbed or contains tautological assertions."""
    errors: list[str] = []
    tier_files = sorted((e2e_dir / "tests").glob("tier*.py"))
    if len(tier_files) != 4:
        errors.append(f"Expected 4 tier*.py modules, found {[p.name for p in tier_files]}")
        return errors

    for path in tier_files:
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in tree.body:
            if not isinstance(node, ast.FunctionDef) or not node.name.startswith("test_"):
                continue
            # Drop optional docstring
            stmts = list(node.body)
            if (
                stmts
                and isinstance(stmts[0], ast.Expr)
                and isinstance(stmts[0].value, ast.Constant)
                and isinstance(stmts[0].value.value, str)
            ):
                stmts = stmts[1:]

            if not stmts:
                errors.append(f"{path.name}:{node.name} has empty body")
                continue
            if len(stmts) == 1:
                only = stmts[0]
                if isinstance(only, ast.Pass):
                    errors.append(f"{path.name}:{node.name} is a 'pass' stub")
                    continue
                if isinstance(only, ast.Return) and (
                    only.value is None
                    or (isinstance(only.value, ast.Constant) and only.value.value in (True, None))
                ):
                    errors.append(f"{path.name}:{node.name} is a trivial return stub")
                    continue
                if (
                    isinstance(only, ast.Expr)
                    and isinstance(only.value, ast.Constant)
                    and only.value.value is Ellipsis
                ):
                    errors.append(f"{path.name}:{node.name} is an Ellipsis stub")
                    continue

            has_real_assert_or_call = False
            for sub in ast.walk(node):
                if isinstance(sub, ast.Assert):
                    if isinstance(sub.test, ast.Constant) and bool(sub.test.value):
                        errors.append(f"{path.name}:{node.name} has tautological assert {sub.test.value!r}")
                    elif (
                        isinstance(sub.test, ast.Compare)
                        and len(sub.test.ops) == 1
                        and isinstance(sub.test.ops[0], ast.Eq)
                        and isinstance(sub.test.left, ast.Constant)
                        and isinstance(sub.test.comparators[0], ast.Constant)
                        and sub.test.left.value == sub.test.comparators[0].value
                    ):
                        errors.append(f"{path.name}:{node.name} has tautological constant equality assert")
                    else:
                        has_real_assert_or_call = True
                elif isinstance(sub, ast.Call):
                    has_real_assert_or_call = True
            if not has_real_assert_or_call:
                errors.append(f"{path.name}:{node.name} has no assertions or exercising calls")
    return errors


def _self_check_offline_harness() -> list[str]:
    """Exercise SQLite JSONB, luminance/contrast, and UIA geometry auditors with positive & negative cases."""
    errors: list[str] = []

    # 1. SQLite JSONB roundtrip matching lib/src/storage/migrations/0.sql
    with tempfile.TemporaryDirectory(prefix="openscq30_selfcheck_") as tmp:
        db = Path(tmp) / "test.sqlite"
        conn = sqlite3.connect(str(db))
        try:
            conn.executescript(
                """
                CREATE TABLE paired_device (
                    mac_address TEXT NOT NULL UNIQUE,
                    model TEXT NOT NULL,
                    is_demo INTEGER NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (strftime('%s'))
                ) STRICT;
                CREATE TABLE quick_preset (
                    device_model TEXT NOT NULL,
                    name TEXT NOT NULL,
                    fields BLOB NOT NULL CHECK(json_valid(fields, 4)),
                    created_at INTEGER NOT NULL DEFAULT (strftime('%s'))
                ) STRICT;
                """
            )
            conn.execute(
                "INSERT INTO paired_device (mac_address, model, is_demo) VALUES ('00:00:00:00:00:03', 'SoundcoreA3028', 1)"
            )
            fields_json = json.dumps([{"settingId": "listeningMode", "value": "NoiseCanceling", "isEnabled": True}])
            conn.execute(
                "INSERT INTO quick_preset (device_model, name, fields) VALUES ('AppleAirPodsPro2', 'Focus', jsonb(?))",
                (fields_json,),
            )
            row = conn.execute("SELECT json(fields) FROM quick_preset WHERE name='Focus'").fetchone()
            if not row or "NoiseCanceling" not in row[0]:
                errors.append(f"SQLite JSONB roundtrip failed: {row}")
        finally:
            conn.close()

    # 2. Luminance & theme verification (both positive and negative checks)
    blank = Image.new("RGBA", (100, 100), (255, 255, 255, 255))
    try:
        verify_theme_luminance(blank, "light")
        errors.append("verify_theme_luminance failed to reject uniform blank white image")
    except AssertionError:
        pass

    light_img = Image.new("RGBA", (200, 100), (245, 245, 247, 255))
    draw_l = ImageDraw.Draw(light_img)
    draw_l.rectangle((16, 16, 184, 84), fill=(255, 255, 255, 255))
    draw_l.rectangle((24, 24, 100, 36), fill=(28, 28, 30, 255))
    verify_theme_luminance(light_img, "light")
    try:
        verify_theme_luminance(light_img, "dark")
        errors.append("verify_theme_luminance failed to reject light image under dark theme")
    except AssertionError:
        pass

    dark_img = Image.new("RGBA", (200, 100), (28, 28, 30, 255))
    draw_d = ImageDraw.Draw(dark_img)
    draw_d.rectangle((16, 16, 184, 84), fill=(44, 44, 46, 255))
    draw_d.rectangle((24, 24, 100, 36), fill=(240, 240, 245, 255))
    verify_theme_luminance(dark_img, "dark")

    # 3. UIA geometry auditor (positive + negative overlap/clipping/offscreen/zero-size checks)
    clean_nodes = [
        {"name": "Add", "controlType": "ControlType.Button", "x": 20, "y": 20, "w": 80, "h": 32, "isOffscreen": False},
        {"name": "Remove", "controlType": "ControlType.Button", "x": 120, "y": 20, "w": 80, "h": 32, "isOffscreen": False},
    ]
    clean_audit = audit_uia_geometry(clean_nodes, (0, 0, 420, 700))
    if not clean_audit.passed:
        errors.append(f"audit_uia_geometry rejected clean layout: {clean_audit}")

    bad_nodes = [
        {"name": "A", "controlType": "ControlType.Button", "x": 20, "y": 20, "w": 80, "h": 32, "isOffscreen": False},
        {"name": "B", "controlType": "ControlType.Button", "x": 60, "y": 20, "w": 80, "h": 32, "isOffscreen": False},
        {"name": "Zero", "controlType": "ControlType.Button", "x": 10, "y": 80, "w": 0, "h": 20, "isOffscreen": False},
        {"name": "Off", "controlType": "ControlType.Button", "x": 10, "y": 110, "w": 50, "h": 20, "isOffscreen": True},
        {"name": "Clip", "controlType": "ControlType.Button", "x": 390, "y": 150, "w": 60, "h": 24, "isOffscreen": False},
    ]
    bad_audit = audit_uia_geometry(bad_nodes, (0, 0, 420, 700))
    if (
        not bad_audit.overlapping_pairs
        or not bad_audit.zero_size_controls
        or not bad_audit.offscreen_controls
        or not bad_audit.out_of_bounds_controls
    ):
        errors.append(f"audit_uia_geometry failed to detect synthetic defects: {bad_audit}")

    return errors


def _cmd_self_check() -> int:
    e2e_dir = REPO_ROOT / "e2e"
    errors: list[str] = []

    # 1. py_compile all .py files in e2e/
    py_files = sorted(e2e_dir.rglob("*.py"))
    with tempfile.TemporaryDirectory(prefix="openscq30_pyc_") as pyc_tmp:
        for idx, pf in enumerate(py_files):
            try:
                cfile = str(Path(pyc_tmp) / f"{idx}.pyc") if sys.dont_write_bytecode else None
                py_compile.compile(str(pf), cfile=cfile, doraise=True)
            except py_compile.PyCompileError as exc:
                errors.append(f"Syntax/compile error in {pf}: {exc}")

    # 2. Registry threshold & uniqueness checks
    tests = all_tests()
    errors.extend(validate_registry_thresholds(tests))

    # 3. AST integrity check (no stubs / no tautological assertions)
    errors.extend(_ast_verify_test_modules(e2e_dir))

    # 4. Fixture integrity checks
    errors.extend(validate_eq_fixtures())
    errors.extend(validate_aap_fixtures())

    # 5. Offline harness checks (SQLite JSONB, luminance, UIA geometry auditor)
    errors.extend(_self_check_offline_harness())

    counts = {t: sum(1 for x in tests if x.tier == t) for t in (1, 2, 3, 4)}
    feat_counts = {
        f"F{i}": {
            "T1": sum(1 for x in tests if x.tier == 1 and x.feature == f"F{i}"),
            "T2": sum(1 for x in tests if x.tier == 2 and x.feature == f"F{i}"),
        }
        for i in range(1, 11)
    }

    if errors:
        print("SELF-CHECK FAILED:")
        for e in errors:
            print(f"  - {e}")
        return 1

    print(
        f"SELF-CHECK PASSED: compiled {len(py_files)} Python files; "
        f"tiers={counts} (total={len(tests)}); features={feat_counts}; "
        f"EQ & AAP fixtures + SQLite JSONB + Visual/UIA auditors verified."
    )
    return 0


def _cmd_dry_run(tests: list[TestCase]) -> int:
    errors = validate_registry_thresholds(all_tests())
    errors.extend(validate_eq_fixtures())
    errors.extend(validate_aap_fixtures())
    if errors:
        print("DRY-RUN VALIDATION FAILED:")
        for e in errors:
            print(f"  - {e}")
        return 1

    print(f"DRY-RUN PLAN ({len(tests)} selected tests):")
    for t in tests:
        reqs = [
            name
            for name, flag in (("cargo", t.requires_cargo), ("cli", t.requires_cli), ("gui", t.requires_gui))
            if flag
        ]
        req_str = ",".join(reqs) if reqs else "fs"
        print(f"  [T{t.tier}] {t.id:10s} ({t.feature}, req={req_str}): {t.name}")
    print("DRY-RUN OK (no binaries invoked).")
    return 0


def _write_json_report(path_str: str | None, payload: dict[str, Any]) -> None:
    if not path_str:
        return
    p = Path(path_str)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def _run_live(tests: list[TestCase], args: argparse.Namespace) -> int:
    if not tests:
        print("ERROR: No tests matched selection filters.")
        return 1

    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    ctx = TestContext(
        repo_root=REPO_ROOT,
        artifacts_dir=Path(args.artifacts_dir),
        run_id=run_id,
        skip_cargo=args.skip_cargo,
        skip_gui=args.skip_gui,
    )

    results: list[dict[str, Any]] = []
    passed = 0
    failed = 0
    skipped = 0

    for idx, tc in enumerate(tests, start=1):
        t0 = time.monotonic()
        status = "PASS"
        detail = ""
        tb = ""
        try:
            out = tc.fn(ctx)
            detail = out or "ok"
            passed += 1
        except TestSkipped as exc:
            status = "SKIP"
            detail = str(exc)
            skipped += 1
        except Exception as exc:
            status = "FAIL"
            detail = f"{type(exc).__name__}: {exc}"
            tb = traceback.format_exc()
            failed += 1

        dur = round(time.monotonic() - t0, 3)
        print(f"[{idx:3d}/{len(tests):3d}] {status:4s} {tc.id:10s} ({dur:6.2f}s) - {detail}")
        if tb and status == "FAIL":
            print(tb.rstrip())
        results.append(
            {
                "id": tc.id,
                "tier": tc.tier,
                "feature": tc.feature,
                "features": list(tc.features),
                "name": tc.name,
                "status": status,
                "duration_s": dur,
                "detail": detail,
                "traceback": tb,
            }
        )

    summary = {
        "run_id": run_id,
        "total": len(tests),
        "passed": passed,
        "failed": failed,
        "skipped": skipped,
        "results": results,
    }
    _write_json_report(args.json_report, summary)
    print(f"\nSUMMARY: total={len(tests)} passed={passed} failed={failed} skipped={skipped}")
    return 0 if failed == 0 else 1


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    if args.self_check:
        return _cmd_self_check()
    selected = _select_tests(args)
    if args.list_tests:
        return _cmd_list_tests(selected)
    if args.dry_run:
        return _cmd_dry_run(selected)
    return _run_live(selected, args)


if __name__ == "__main__":
    raise SystemExit(main())
