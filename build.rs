use std::path::{Path, PathBuf};

/// GLSL sources and their precompiled SPIR-V counterparts. The SPIR-V is
/// checked in so that no platform needs a C++ shader toolchain at build time;
/// regenerate it with `tools/compile-shaders.sh` after editing a shader.
const SHADERS: [&str; 7] = [
    "world.vert",
    "world.frag",
    "gui.vert",
    "gui.frag",
    "panorama.vert",
    "panorama.frag",
    "panorama_blur.frag",
];

fn main() {
    let shader_dir = Path::new("src/vulkan/shaders");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));

    for file in SHADERS {
        let spirv_path = shader_dir.join("spv").join(format!("{file}.spv"));
        let binary = std::fs::read(&spirv_path).unwrap_or_else(|error| {
            panic!(
                "failed reading precompiled SPIR-V {}: {error} — regenerate it with \
                 tools/compile-shaders.sh",
                spirv_path.display()
            )
        });
        let output_path = out_dir.join(format!("{file}.spv"));
        std::fs::write(&output_path, binary).unwrap_or_else(|error| {
            panic!("failed writing {}: {error}", output_path.display())
        });

        // Recompile the build script itself when the GLSL changes so the
        // staleness check below runs again.
        let glsl_path = shader_dir.join(file);
        println!("cargo:rerun-if-changed={}", spirv_path.display());
        if glsl_path.is_file() {
            println!("cargo:rerun-if-changed={}", glsl_path.display());
            let newer_glsl = std::fs::metadata(&glsl_path)
                .and_then(|glsl| glsl.modified())
                .ok()
                .zip(std::fs::metadata(&spirv_path).and_then(|spirv| spirv.modified()).ok())
                .is_some_and(|(glsl, spirv)| glsl > spirv);
            if newer_glsl {
                println!(
                    "cargo:warning={} is newer than src/vulkan/shaders/spv/{}.spv; run \
                     tools/compile-shaders.sh to refresh the checked-in SPIR-V",
                    glsl_path.display(),
                    file
                );
            }
        }
    }
}
