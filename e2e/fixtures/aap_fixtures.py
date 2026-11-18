"""Authoritative FPNDev/wpods AirPods Pro 2 AAP & Windows pipe fixtures."""

from __future__ import annotations

APPLE_VID = 0x004C
AIRPODS_PRO_2_PIDS: tuple[str, ...] = ("2014", "2024")
WPODS_PIPE_PATH = r"\\.\pipe\WPods.Aap"
WPODS_SERVICE_NAME = "WPods.Service"
BTHENUM_SERVICE_GUID = "{74EC2172-0BAD-4D01-8F77-997B2BE0722A}"
BTHENUM_PREFIX = rf"BTHENUM\{BTHENUM_SERVICE_GUID}"

AAP_HEADER = bytes([0x04, 0x00, 0x04, 0x00])
AAP_HANDSHAKE_PACKET = bytes.fromhex("00000400010002000000000000000000")
MAX_AAP_PACKET_BYTES = 1691

PIPE_OPEN_PREFIX = b"AP"
PIPE_INBOUND_AAP_DATA = 0x00
PIPE_INBOUND_CHANNEL_CLOSED = 0x01
PIPE_INBOUND_OPEN_STATUS = 0x02
PIPE_INBOUND_CALL_ACTIVE = 0x03

PIPE_STATUS_ERRORS: dict[int, str] = {
    0: "OK",
    121: "ERROR_SEM_TIMEOUT",
    170: "ERROR_BUSY",
    1167: "ERROR_DEVICE_NOT_CONNECTED",
    1225: "ERROR_CONNECTION_REFUSED",
}

DEMO_Q30_MAC = "00:00:00:00:00:03"
DEMO_APP2_MAC = "00:00:00:00:00:29"
REAL_APP2_MAC = "AA:BB:CC:DD:EE:FF"
UNPAIRED_MAC = "DE:AD:BE:EF:00:01"

LISTENING_MODES: tuple[str, ...] = ("Off", "NoiseCanceling", "Transparency", "Adaptive")
PRESS_SPEED_OPTIONS: tuple[str, ...] = ("Default", "Slower", "Slowest")
HOLD_DURATION_OPTIONS: tuple[str, ...] = ("Default", "Shorter", "Shortest")
MIC_SIDE_OPTIONS: tuple[str, ...] = ("Automatic", "Right", "Left")
APPLE_APP2_MODEL_NUMBERS: tuple[str, ...] = ("A2931", "A2699", "A2698", "A3047", "A3048", "A3049")

# Dynamic SettingId alias map for opaque-box CLI discovery via `list-settings --json`
APP2_SETTING_ALIASES: dict[str, tuple[str, ...]] = {
    "listening_mode": ("listeningMode", "ambientSoundMode"),
    "allow_off": ("allowOff", "allowOffListeningMode", "allowOffMode"),
    "one_bud_anc": ("oneBudAnc", "oneBudNoiseCanceling", "singleBudAnc"),
    "stem_cycle_modes": ("stemCycleModes", "stemModes", "listeningModeCycle", "pressAndHoldCycleModes"),
    "press_speed": ("pressSpeed", "stemPressSpeed"),
    "hold_duration": ("holdDuration", "pressAndHoldDuration", "stemHoldDuration"),
    "tone_volume": ("toneVolume",),
    "conversation_awareness": ("conversationAwareness", "conversationalAwareness"),
    "personalized_volume": ("personalizedVolume",),
    "mic_side": ("micSide", "microphoneSide", "microphone"),
    "wearing_detection": ("wearingDetection", "automaticEarDetection"),
    "battery_left": ("batteryLevelLeft",),
    "battery_right": ("batteryLevelRight",),
    "battery_case": ("caseBatteryLevel", "batteryLevelCase"),
    "charging_left": ("isChargingLeft",),
    "charging_right": ("isChargingRight",),
    "ear_left": ("earDetectionLeft", "leftEarDetection", "earStatusLeft"),
    "ear_right": ("earDetectionRight", "rightEarDetection", "earStatusRight"),
    "serial_number": ("serialNumber",),
    "firmware_version": ("firmwareVersion",),
    "model_number": ("modelNumber", "appleModelNumber"),
}


def mac_to_u64(mac: str) -> int:
    """Convert colon-separated MAC to Windows 48-bit u64 integer."""
    parts = mac.split(":")
    if len(parts) != 6:
        raise ValueError(f"Invalid MAC address: {mac!r}")
    return int("".join(parts), 16)


def encode_pipe_open_frame(mac: str) -> bytes:
    """Encode 10-byte WPods.Aap open request: 'AP' + u64 LE MAC."""
    return PIPE_OPEN_PREFIX + mac_to_u64(mac).to_bytes(8, "little", signed=False)


def encode_pipe_status_frame(win32_error: int) -> bytes:
    """Encode 5-byte WPods.Aap open status response: 0x02 + i32 LE."""
    return bytes([PIPE_INBOUND_OPEN_STATUS]) + int(win32_error).to_bytes(4, "little", signed=True)


def encode_tone_volume_packet(volume: int) -> bytes:
    """Encode AAP 0x09 0x1F tone volume packet; byte 8 MUST be 0x50."""
    if volume < 0 or volume > 100:
        raise ValueError(f"Tone volume out of 0..=100 range: {volume}")
    return AAP_HEADER + bytes([0x09, 0x00, 0x1F, volume, 0x50, 0x00, 0x00])


def flatten_list_settings(list_settings_json: list | dict) -> list[dict]:
    """Flatten `openscq30 device list-settings --json` output into setting dicts."""
    if isinstance(list_settings_json, dict):
        # --no-categories returns map[settingId] -> setting_obj
        out = []
        for sid, obj in list_settings_json.items():
            entry = dict(obj)
            entry.setdefault("settingId", sid)
            out.append(entry)
        return out
    flat: list[dict] = []
    for cat in list_settings_json:
        cat_id = cat.get("categoryId")
        for s in cat.get("settings", []):
            entry = dict(s)
            entry.setdefault("categoryId", cat_id)
            flat.append(entry)
    return flat


def resolve_app2_setting(flat_settings: list[dict], canonical_key: str) -> dict | None:
    """Resolve an AirPods Pro 2 setting dict dynamically using known SettingId aliases."""
    aliases = APP2_SETTING_ALIASES.get(canonical_key, (canonical_key,))
    by_lower = {s.get("settingId", "").lower(): s for s in flat_settings}
    for alias in aliases:
        hit = by_lower.get(alias.lower())
        if hit is not None:
            return hit
    # Substring fallback on camelCase tokens
    tokens = [t.lower() for t in canonical_key.split("_") if t]
    for s in flat_settings:
        sid_lower = s.get("settingId", "").lower()
        if all(tok in sid_lower for tok in tokens):
            return s
    return None


def validate_aap_fixtures() -> list[str]:
    """Validate AAP wire encoding helpers and constants."""
    errors: list[str] = []
    open_frame = encode_pipe_open_frame("AA:BB:CC:DD:EE:FF")
    if len(open_frame) != 10 or open_frame[:2] != b"AP":
        errors.append(f"Invalid pipe open frame: {open_frame!r}")
    status_ok = encode_pipe_status_frame(0)
    if status_ok != b"\x02\x00\x00\x00\x00":
        errors.append(f"Invalid status OK frame: {status_ok!r}")
    status_1167 = encode_pipe_status_frame(1167)
    if len(status_1167) != 5 or int.from_bytes(status_1167[1:], "little", signed=True) != 1167:
        errors.append(f"Invalid status 1167 frame: {status_1167!r}")
    tone_pkt = encode_tone_volume_packet(80)
    if len(tone_pkt) != 11 or tone_pkt[:4] != AAP_HEADER or tone_pkt[7] != 80 or tone_pkt[8] != 0x50:
        errors.append(f"Invalid tone volume packet: {tone_pkt.hex()}")
    if len(AAP_HANDSHAKE_PACKET) != 16:
        errors.append(f"Expected 16-byte handshake packet, got {len(AAP_HANDSHAKE_PACKET)}")
    return errors
