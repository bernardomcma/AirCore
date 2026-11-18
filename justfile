mod cli
mod gui
mod i18n
mod i18n-macros
mod lib
mod lib-macros
mod lib-has

build-output-dir := "build-output"

[default]
[doc("List available recipes")]
list:
    @just --list

[doc("Run a fully optimized release build")]
[group("build")]
build-gui features='': create-build-output-dir
    just gui::build release '{{ features }}'
    cp target/release/openscq30-gui.exe '{{ build-output-dir }}/'

[doc("Run a release build with fast LTO")]
[group("build")]
build-gui-fast features='': create-build-output-dir
    just gui::build release-fast '{{ features }}'
    cp target/release-fast/openscq30-gui.exe '{{ build-output-dir }}/'

[doc("Build the Windows installer. The GUI must be built first.")]
[group("build")]
[windows]
build-gui-installer: create-build-output-dir
    ./packaging/windows/build.sh
    cp packaging/windows/Output/openscq30-gui-installer.exe '{{ build-output-dir }}/'

[doc("Run a fully optimized CLI release build")]
[group("build")]
build-cli features='': create-build-output-dir
    just cli::build release '{{ features }}'
    cp target/release/openscq30.exe '{{ build-output-dir }}/'

[doc("Run a CLI release build with fast LTO")]
[group("build")]
build-cli-fast features='': create-build-output-dir
    just cli::build release-fast '{{ features }}'
    cp target/release-fast/openscq30.exe '{{ build-output-dir }}/'

[group("build")]
[private]
create-build-output-dir:
    mkdir -p build-output

[doc("Run all tests")]
test: lib::test cli::test gui::test

test-cov: lib::test-cov cli::test-cov gui::test-cov

llvm-cov-clean:
    cargo llvm-cov clean --workspace

alias fmt := format

[parallel]
format: cli::format gui::format i18n::format i18n-macros::format lib::format lib-macros::format lib-has::format

[parallel]
format-check: cli::format-check gui::format-check i18n::format-check i18n-macros::format-check lib::format-check lib-macros::format-check lib-has::format-check
