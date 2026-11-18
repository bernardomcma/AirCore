"""Isolated CLI, cargo, rustc, and rustup execution driver for OpenSCQ30 E2E tests."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


class MissingPrerequisiteError(RuntimeError):
    """Raised when required toolchain or compiled binary is missing during a live run."""


@dataclass(frozen=True)
class CommandResult:
    """Captured result from a subprocess invocation."""

    cmd: list[str]
    returncode: int
    stdout: str
    stderr: str
    duration_s: float


def _extra_cargo_bin_dir() -> Path:
    return Path.home() / ".cargo" / "bin"


def find_toolchain_exe(name: str) -> str:
    """Locate rustc/cargo/rustup on PATH or ~/.cargo/bin; fail loudly if absent."""
    candidates = [name, f"{name}.exe"] if not name.endswith(".exe") else [name]
    for cand in candidates:
        found = shutil.which(cand)
        if found:
            return found
    cargo_bin = _extra_cargo_bin_dir()
    for cand in candidates:
        p = cargo_bin / cand
        if p.is_file():
            return str(p)
    raise MissingPrerequisiteError(
        f"Required toolchain executable '{name}' not found on PATH or in {cargo_bin}"
    )


def _augmented_env(extra_env: dict[str, str] | None = None) -> dict[str, str]:
    env = os.environ.copy()
    cargo_bin_dir = _extra_cargo_bin_dir()
    if cargo_bin_dir.is_dir():
        cargo_bin = str(cargo_bin_dir)
        path_key = next((k for k in env if k.upper() == "PATH"), "PATH")
        current_path = env.get(path_key, "")
        existing_entries = [os.path.normcase(p) for p in current_path.split(os.pathsep) if p]
        if os.path.normcase(cargo_bin) not in existing_entries:
            env[path_key] = f"{cargo_bin}{os.pathsep}{current_path}" if current_path else cargo_bin
    if extra_env:
        env.update(extra_env)
    return env


def run_command(
    cmd: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    timeout: int = 120,
) -> CommandResult:
    """Execute a subprocess with timeout and UTF-8 decoding."""
    t0 = time.monotonic()
    proc = subprocess.run(
        cmd,
        cwd=str(cwd) if cwd else None,
        env=_augmented_env(env),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    return CommandResult(
        cmd=list(cmd),
        returncode=proc.returncode,
        stdout=proc.stdout,
        stderr=proc.stderr,
        duration_s=round(time.monotonic() - t0, 3),
    )


def run_cargo(
    repo_root: Path,
    args: list[str],
    *,
    env: dict[str, str] | None = None,
    timeout: int = 600,
) -> CommandResult:
    """Run cargo in repo_root; fails loudly if cargo is missing."""
    cargo = find_toolchain_exe("cargo")
    return run_command([cargo, *args], cwd=repo_root, env=env, timeout=timeout)


def find_cli_binary(repo_root: Path) -> Path | None:
    """Discover compiled openscq30.exe in standard Cargo target profiles."""
    for rel in (
        Path("target/release-fast/openscq30.exe"),
        Path("target/debug/openscq30.exe"),
        Path("target/release/openscq30.exe"),
    ):
        p = repo_root / rel
        if p.is_file() and p.stat().st_size > 0:
            return p
    return None


def resolve_cli_command(repo_root: Path) -> list[str]:
    """Return command prefix to invoke openscq30 CLI (compiled binary or cargo run)."""
    exe = find_cli_binary(repo_root)
    if exe is not None:
        return [str(exe)]
    cargo = find_toolchain_exe("cargo")
    return [cargo, "run", "-q", "-p", "openscq30-cli", "--"]


def find_gui_binary(repo_root: Path) -> Path | None:
    """Discover compiled openscq30-gui.exe in standard Cargo target profiles."""
    for rel in (
        Path("target/release-fast/openscq30-gui.exe"),
        Path("target/debug/openscq30-gui.exe"),
        Path("target/release/openscq30-gui.exe"),
    ):
        p = repo_root / rel
        if p.is_file() and p.stat().st_size > 0:
            return p
    return None


def ensure_cli_built(repo_root: Path) -> Path:
    """Return existing openscq30.exe or build it via cargo."""
    exe = find_cli_binary(repo_root)
    if exe is not None:
        return exe
    res = run_cargo(repo_root, ["build", "--bin", "openscq30"], timeout=600)
    if res.returncode != 0:
        raise AssertionError(
            f"Failed to build openscq30 CLI (rc={res.returncode}):\n{res.stderr}\n{res.stdout}"
        )
    exe = find_cli_binary(repo_root)
    if exe is None:
        raise MissingPrerequisiteError("cargo build --bin openscq30 succeeded but binary was not found")
    return exe


def ensure_gui_built(repo_root: Path) -> Path:
    """Return existing openscq30-gui.exe or build it via cargo."""
    exe = find_gui_binary(repo_root)
    if exe is not None:
        return exe
    res = run_cargo(repo_root, ["build", "-p", "openscq30-gui"], timeout=600)
    if res.returncode != 0:
        raise AssertionError(
            f"Failed to build openscq30-gui (rc={res.returncode}):\n{res.stderr}\n{res.stdout}"
        )
    exe = find_gui_binary(repo_root)
    if exe is None:
        raise MissingPrerequisiteError("cargo build -p openscq30-gui succeeded but binary was not found")
    return exe


class IsolatedCliSession:
    """Per-test isolated openscq30 CLI session using OPENSCQ30_DATABASE_PATH."""

    def __init__(self, repo_root: Path, db_path: Path | None = None) -> None:
        self.repo_root = repo_root
        self._tmpdir: tempfile.TemporaryDirectory[str] | None = None
        if db_path is None:
            self._tmpdir = tempfile.TemporaryDirectory(prefix="openscq30_e2e_cli_")
            self.db_path = Path(self._tmpdir.name) / "database.sqlite"
        else:
            self.db_path = db_path
        self._cmd_prefix: list[str] | None = None

    def __enter__(self) -> IsolatedCliSession:
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        if self._tmpdir is not None:
            self._tmpdir.cleanup()

    def _prefix(self) -> list[str]:
        if self._cmd_prefix is None:
            self._cmd_prefix = resolve_cli_command(self.repo_root)
        return self._cmd_prefix

    def _default_timeout(self) -> int:
        prefix = self._prefix()
        return 180 if Path(prefix[0]).stem.lower() == "cargo" else 60

    def run(
        self,
        args: list[str],
        *,
        expect_ok: bool = True,
        timeout: int | None = None,
        extra_env: dict[str, str] | None = None,
    ) -> CommandResult:
        """Execute openscq30 with OPENSCQ30_DATABASE_PATH pinned to this session's DB."""
        env = {"OPENSCQ30_DATABASE_PATH": str(self.db_path)}
        if extra_env:
            env.update(extra_env)
        eff_timeout = self._default_timeout() if timeout is None else timeout
        res = run_command([*self._prefix(), *args], cwd=self.repo_root, env=env, timeout=eff_timeout)
        if expect_ok and res.returncode != 0:
            raise AssertionError(
                f"CLI command failed (rc={res.returncode}): {' '.join(res.cmd)}\n"
                f"stdout: {res.stdout}\nstderr: {res.stderr}"
            )
        return res

    def run_json(self, args: list[str], *, timeout: int | None = None) -> Any:
        """Run CLI command expecting exit code 0 and valid JSON stdout."""
        res = self.run(args, expect_ok=True, timeout=timeout)
        try:
            return json.loads(res.stdout)
        except json.JSONDecodeError as exc:
            raise AssertionError(
                f"CLI stdout was not valid JSON for {' '.join(res.cmd)}: {exc}\n"
                f"stdout={res.stdout!r}\nstderr={res.stderr!r}"
            ) from exc

    def list_models(self) -> list[dict[str, Any]]:
        data = self.run_json(["list-models", "--json"])
        assert isinstance(data, list), f"Expected list from list-models --json, got {type(data)}"
        return data

    def pair_device(self, mac: str, model: str, *, demo: bool = True) -> CommandResult:
        args = ["paired-devices", "add", "--mac-address", mac, "--model", model]
        if demo:
            args.append("--demo")
        res = self.run(args, expect_ok=True)
        assert "Paired" in res.stdout, f"Expected 'Paired' in stdout, got {res.stdout!r}"
        return res

    def unpair_device(self, mac: str) -> CommandResult:
        res = self.run(["paired-devices", "remove", "--mac-address", mac], expect_ok=True)
        assert "Unpaired" in res.stdout, f"Expected 'Unpaired' in stdout, got {res.stdout!r}"
        return res

    def paired_devices(self) -> list[dict[str, Any]]:
        data = self.run_json(["paired-devices", "list", "--json"])
        assert isinstance(data, list), f"Expected list from paired-devices list --json, got {type(data)}"
        return data

    def list_settings(
        self,
        mac: str,
        *,
        no_categories: bool = False,
        no_extended_info: bool = False,
    ) -> list[dict[str, Any]] | dict[str, Any]:
        args = ["device", "--mac-address", mac, "list-settings", "--json"]
        if no_categories:
            args.append("--no-categories")
        if no_extended_info:
            args.append("--no-extended-info")
        return self.run_json(args)

    def setting_ops(
        self,
        mac: str,
        ops: list[tuple[str, str, str | None]],
        *,
        expect_ok: bool = True,
    ) -> tuple[CommandResult, list[dict[str, Any]] | None]:
        """Run single-process ordered --set / --get operations.

        ops elements: ("set", setting_id, value_or_none) or ("get", setting_id, None)
        """
        args = ["device", "--mac-address", mac, "setting", "--json"]
        for kind, sid, val in ops:
            if kind == "set":
                arg = f"{sid}={val}" if val is not None else sid
                args.extend(["--set", arg])
            elif kind == "get":
                args.extend(["--get", sid])
            else:
                raise ValueError(f"Unsupported op kind: {kind!r}")
        res = self.run(args, expect_ok=expect_ok)
        if not expect_ok:
            return res, None
        parsed = json.loads(res.stdout) if res.stdout.strip() else []
        return res, parsed
