"""Authoritative pre-existing Soundcore EQ preset baselines and R2 genre contracts."""

from __future__ import annotations

from dataclasses import dataclass

COMMON_BAND_HZ: tuple[int, ...] = (100, 200, 400, 800, 1600, 3200, 6400, 12800)
COMMON_PRESET_MIN_VOLUME = -60
COMMON_PRESET_MAX_VOLUME = 60
COMMON_CUSTOM_MIN_VOLUME = -120
COMMON_CUSTOM_MAX_VOLUME = 134
COMMON_FRACTION_DIGITS = 1

# Mandatory genres required by ORIGINAL_REQUEST.md §R2
MANDATORY_GENRE_OPTIONS: tuple[str, ...] = (
    "Trap",
    "HipHop",
    "Rock",
    "Techno",
    "HardTechno",
)


@dataclass(frozen=True)
class EqPresetBaseline:
    """Single pre-existing EQ preset specification."""

    id: int
    option: str
    ftl_key: str
    localized_en: str
    curve: tuple[int, ...]


# Exact 22 pre-existing presets from lib/src/devices/soundcore/common/modules/equalizer.rs
BASELINE_PRESETS: tuple[EqPresetBaseline, ...] = (
    EqPresetBaseline(0x0000, "SoundcoreSignature", "soundcore-signature", "Soundcore Signature", (0, 0, 0, 0, 0, 0, 0, 0)),
    EqPresetBaseline(0x0001, "Acoustic", "acoustic", "Acoustic", (40, 10, 20, 20, 40, 40, 40, 20)),
    EqPresetBaseline(0x0002, "BassBooster", "bass-booster", "Bass Booster", (40, 30, 10, 0, 0, 0, 0, 0)),
    EqPresetBaseline(0x0003, "BassReducer", "bass-reducer", "Bass Reducer", (-40, -30, -10, 0, 0, 0, 0, 0)),
    EqPresetBaseline(0x0004, "Classical", "classical", "Classical", (30, 30, -20, -20, 0, 20, 30, 40)),
    EqPresetBaseline(0x0005, "Podcast", "podcast", "Podcast", (-30, 20, 40, 40, 30, 20, 0, -20)),
    EqPresetBaseline(0x0006, "Dance", "dance", "Dance", (20, -30, -10, 10, 20, 20, 10, -30)),
    EqPresetBaseline(0x0007, "Deep", "deep", "Deep", (20, 10, 30, 30, 20, -20, -40, -50)),
    EqPresetBaseline(0x0008, "Electronic", "electronic", "Electronic", (30, 20, -20, 20, 10, 20, 30, 30)),
    EqPresetBaseline(0x0009, "Flat", "flat", "Flat", (-20, -20, -10, 0, 0, 0, -20, -20)),
    EqPresetBaseline(0x000A, "HipHop", "hip-hop", "Hip Hop", (20, 30, -10, -10, 20, -10, 20, 30)),
    EqPresetBaseline(0x000B, "Jazz", "jazz", "Jazz", (20, 20, -20, -20, 0, 20, 30, 40)),
    EqPresetBaseline(0x000C, "Latin", "latin", "Latin", (0, 0, -20, -20, -20, 0, 30, 50)),
    EqPresetBaseline(0x000D, "Lounge", "lounge", "Lounge", (-10, 20, 40, 30, 0, -20, 20, 10)),
    EqPresetBaseline(0x000E, "Piano", "piano", "Piano", (0, 30, 30, 20, 40, 50, 30, 40)),
    EqPresetBaseline(0x000F, "Pop", "pop", "Pop", (-10, 10, 30, 30, 10, -10, -20, -30)),
    EqPresetBaseline(0x0010, "RnB", "rnb", "RnB", (60, 20, -20, -20, 20, 30, 30, 40)),
    EqPresetBaseline(0x0011, "Rock", "rock", "Rock", (30, 20, -10, -10, 10, 30, 30, 30)),
    EqPresetBaseline(0x0012, "SmallSpeakers", "small-speakers", "Small Speakers", (40, 30, 10, 0, -20, -30, -40, -40)),
    EqPresetBaseline(0x0013, "SpokenWord", "spoken-word", "Spoken Word", (-30, -20, 10, 20, 20, 10, 0, -30)),
    EqPresetBaseline(0x0014, "TrebleBooster", "treble-booster", "Treble Booster", (-20, -20, -20, -10, 10, 20, 20, 40)),
    EqPresetBaseline(0x0015, "TrebleReducer", "treble-reducer", "Treble Reducer", (0, 0, 0, -20, -30, -40, -40, -60)),
)

BASELINE_BY_OPTION: dict[str, EqPresetBaseline] = {p.option: p for p in BASELINE_PRESETS}
BASELINE_OPTION_NAMES: tuple[str, ...] = tuple(p.option for p in BASELINE_PRESETS)

# Soundcore common_settings_type_2() (used by A3949 etc.) overrides Rock curve
TYPE2_PRESET_OVERRIDES: dict[str, tuple[int, ...]] = {
    "Rock": (30, 20, -10, -10, 10, 30, 40, 50),
}

# SoundcoreA3944 overrides BassBooster curve
A3944_PRESET_OVERRIDES: dict[str, tuple[int, ...]] = {
    "BassBooster": (50, 50, 30, 10, 10, 20, -10, -20),
}

# Bespoke preset shared by SoundcoreA3876 and SoundcoreD1101
VOLUME_BOOSTER_CURVE: tuple[int, ...] = (20, 30, 40, 50, 60, 60, 50, 40)


def expected_type2_curves() -> dict[str, tuple[int, ...]]:
    """Return expected 8-band curves for common_settings_type_2()."""
    curves = {p.option: p.curve for p in BASELINE_PRESETS}
    curves.update(TYPE2_PRESET_OVERRIDES)
    return curves


def validate_eq_fixtures() -> list[str]:
    """Validate internal invariants of EQ baseline fixtures."""
    errors: list[str] = []
    if len(BASELINE_PRESETS) != 22:
        errors.append(f"Expected 22 baseline presets, got {len(BASELINE_PRESETS)}")
    ids = [p.id for p in BASELINE_PRESETS]
    if ids != list(range(22)):
        errors.append(f"Baseline preset IDs must be 0..21, got {ids}")
    options = [p.option for p in BASELINE_PRESETS]
    if len(set(o.lower() for o in options)) != len(options):
        errors.append("Duplicate case-insensitive option names in BASELINE_PRESETS")
    curves = [p.curve for p in BASELINE_PRESETS]
    if len(set(curves)) != len(curves):
        errors.append("Duplicate curves found in BASELINE_PRESETS")
    for p in BASELINE_PRESETS:
        if len(p.curve) != 8:
            errors.append(f"{p.option}: expected 8 bands, got {len(p.curve)}")
        if any(v < COMMON_PRESET_MIN_VOLUME or v > COMMON_PRESET_MAX_VOLUME for v in p.curve):
            errors.append(f"{p.option}: curve {p.curve} out of [-60, 60] bounds")
    type2_curves = list(expected_type2_curves().values())
    if len(set(type2_curves)) != len(type2_curves):
        errors.append("Duplicate curves found in common_settings_type_2() overrides")
    return errors
