"""Test registry, metadata, filtering, and threshold validation."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

VALID_TIERS = (1, 2, 3, 4)
VALID_FEATURES = tuple(f"F{i}" for i in range(1, 11))


class TestSkipped(Exception):
    """Raised ONLY when an explicit skip flag (--skip-cargo / --skip-gui) excludes a prerequisite."""


@dataclass
class TestContext:
    """Execution context passed to every E2E test function."""

    repo_root: Path
    artifacts_dir: Path
    run_id: str
    skip_cargo: bool = False
    skip_gui: bool = False
    shared_state: dict = field(default_factory=dict)


@dataclass(frozen=True)
class TestCase:
    """Single registered opaque-box E2E test case."""

    id: str
    tier: int
    feature: str
    features: tuple[str, ...]
    name: str
    description: str
    fn: Callable[[TestContext], str | None]
    requires_cargo: bool = False
    requires_gui: bool = False
    requires_cli: bool = False


_REGISTRY: list[TestCase] = []
_REGISTRY_IDS: set[str] = set()


def register_test(
    *,
    id: str,
    tier: int,
    feature: str,
    name: str,
    description: str,
    features: tuple[str, ...] | None = None,
    requires_cargo: bool = False,
    requires_gui: bool = False,
    requires_cli: bool = False,
) -> Callable[[Callable[[TestContext], str | None]], Callable[[TestContext], str | None]]:
    """Decorator registering an E2E test case with strict schema checks."""
    if not id or not id.strip():
        raise ValueError("TestCase.id must be non-empty")
    if tier not in VALID_TIERS:
        raise ValueError(f"Invalid tier {tier!r} for {id}; expected one of {VALID_TIERS}")
    if feature not in VALID_FEATURES:
        raise ValueError(f"Invalid feature {feature!r} for {id}; expected F1..F10")
    feat_tuple = features if features is not None else (feature,)
    for feat in feat_tuple:
        if feat not in VALID_FEATURES:
            raise ValueError(f"Invalid feature {feat!r} in features for {id}")
    if feature not in feat_tuple:
        feat_tuple = (feature, *feat_tuple)
    if not name or not name.strip():
        raise ValueError(f"TestCase.name must be non-empty for {id}")
    if not description or not description.strip():
        raise ValueError(f"TestCase.description must be non-empty for {id}")

    def decorator(fn: Callable[[TestContext], str | None]) -> Callable[[TestContext], str | None]:
        if id in _REGISTRY_IDS:
            raise ValueError(f"Duplicate test id registered: {id}")
        _REGISTRY_IDS.add(id)
        _REGISTRY.append(
            TestCase(
                id=id,
                tier=tier,
                feature=feature,
                features=feat_tuple,
                name=name,
                description=description,
                fn=fn,
                requires_cargo=requires_cargo,
                requires_gui=requires_gui,
                requires_cli=requires_cli,
            )
        )
        return fn

    return decorator


def all_tests() -> list[TestCase]:
    """Return a copy of all registered tests."""
    return list(_REGISTRY)


def filter_tests(
    *,
    tier: int | None = None,
    feature: str | None = None,
    test_id: str | None = None,
) -> list[TestCase]:
    """Filter registered tests by tier, feature, or exact test ID."""
    tests = all_tests()
    if tier is not None:
        tests = [t for t in tests if t.tier == tier]
    if feature is not None:
        norm_feat = feature.upper()
        tests = [t for t in tests if t.feature == norm_feat or norm_feat in t.features]
    if test_id is not None:
        tests = [t for t in tests if t.id == test_id]
    return tests


def validate_registry_thresholds(tests: list[TestCase] | None = None) -> list[str]:
    """Verify mandatory E2E coverage thresholds from TEST_INFRA.md and SCOPE.md."""
    items = all_tests() if tests is None else tests
    errors: list[str] = []

    seen: set[str] = set()
    for tc in items:
        if tc.id in seen:
            errors.append(f"Duplicate test ID: {tc.id}")
        seen.add(tc.id)
        if not tc.description.strip():
            errors.append(f"Empty description on {tc.id}")

    for feat in VALID_FEATURES:
        t1_count = sum(1 for t in items if t.tier == 1 and t.feature == feat)
        t2_count = sum(1 for t in items if t.tier == 2 and t.feature == feat)
        if t1_count < 5:
            errors.append(f"Feature {feat} has {t1_count} Tier 1 tests (minimum 5 required)")
        if t2_count < 5:
            errors.append(f"Feature {feat} has {t2_count} Tier 2 tests (minimum 5 required)")

    t3_count = sum(1 for t in items if t.tier == 3)
    t4_count = sum(1 for t in items if t.tier == 4)
    if t3_count < 10:
        errors.append(f"Tier 3 has {t3_count} tests (minimum 10 required)")
    if t4_count < 5:
        errors.append(f"Tier 4 has {t4_count} tests (minimum 5 required)")

    # Verify every feature F1..F10 appears in Tier 3 and Tier 4
    t3_features = {f for t in items if t.tier == 3 for f in t.features}
    t4_features = {f for t in items if t.tier == 4 for f in t.features}
    for feat in VALID_FEATURES:
        if feat not in t3_features:
            errors.append(f"Feature {feat} has no Tier 3 coverage")
        if feat not in t4_features:
            errors.append(f"Feature {feat} has no Tier 4 coverage")

    return errors
