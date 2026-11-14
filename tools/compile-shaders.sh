#!/bin/sh
# Recompiles src/vulkan/shaders/*.{vert,frag} into the checked-in SPIR-V that
# every platform consumes (src/vulkan/shaders/spv/*.spv).
#
# Requires glslangValidator (glslang): `pkg install glslang` on Termux,
# `brew install glslang` on macOS, `apt install glslang-tools` on Debian.
#
# Why checked-in SPIR-V: shaderc has to build a C++ toolchain for the *target*,
# which breaks cross-compilation to iOS and slows every desktop build. The
# runtime artifacts are identical because both shaderc and glslangValidator
# compile the same GLSL through glslang.
set -eu

cd "$(dirname "$0")/.."

if ! command -v glslangValidator >/dev/null 2>&1; then
    echo "glslangValidator not found; install glslang first" >&2
    exit 1
fi

mkdir -p src/vulkan/shaders/spv
for source in src/vulkan/shaders/*.vert src/vulkan/shaders/*.frag; do
    name=$(basename "$source")
    echo "compiling $name"
    glslangValidator -V --target-env vulkan1.1 -o "src/vulkan/shaders/spv/$name.spv" "$source"
done
echo "done: $(ls src/vulkan/shaders/spv | wc -l) SPIR-V modules"
