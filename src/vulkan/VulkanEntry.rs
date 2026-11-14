//! Platform-specific acquisition of the Vulkan entry point.
//!
//! Desktop builds link the Vulkan loader directly (`ash` feature `linked`), so
//! the entry point is the one exported by the system loader. iOS has no system
//! Vulkan loader: the client ships `MoltenVK.framework` inside the app bundle
//! and must dlopen it (`ash` feature `loaded`) before any Vulkan call.

/// Returns a Vulkan entry point for the current platform.
///
/// On iOS this searches the usual in-bundle locations for MoltenVK, including
/// the `Frameworks` directory next to the executable, `@executable_path`,
/// `@rpath` and the plain dynamic-loader names. The first candidate that loads
/// wins; if none does, the error lists every attempt so the failure is visible
/// in `Documents/latest.log` on device.
pub fn create() -> anyhow::Result<ash::Entry> {
    #[cfg(not(target_os = "ios"))]
    {
        // Linking the loader guarantees `vkGetInstanceProcAddr` is present.
        Ok(ash::Entry::linked())
    }

    #[cfg(target_os = "ios")]
    {
        use anyhow::anyhow;

        let mut candidates: Vec<String> = Vec::new();
        if let Ok(executable) = std::env::current_exe() {
            log::info!("client executable: {}", executable.display());
            if let Some(directory) = executable.parent() {
                let frameworks = directory.join("Frameworks");
                candidates.push(
                    frameworks
                        .join("MoltenVK.framework")
                        .join("MoltenVK")
                        .display()
                        .to_string(),
                );
                candidates.push(frameworks.join("libMoltenVK.dylib").display().to_string());
                candidates.push(directory.join("libMoltenVK.dylib").display().to_string());
            }
        }
        // Dyld-style paths keep working when the framework is embedded with a
        // standard `@rpath` install name instead of an absolute bundle path.
        candidates.push("@executable_path/Frameworks/MoltenVK.framework/MoltenVK".to_owned());
        candidates.push("@rpath/MoltenVK.framework/MoltenVK".to_owned());
        candidates.push("@executable_path/Frameworks/libMoltenVK.dylib".to_owned());
        candidates.push("libMoltenVK.dylib".to_owned());

        let mut attempts: Vec<String> = Vec::new();
        for candidate in &candidates {
            match unsafe { ash::Entry::load_from(candidate) } {
                Ok(entry) => {
                    log::info!("MoltenVK loaded from {candidate}");
                    return Ok(entry);
                }
                Err(error) => attempts.push(format!("{candidate}: {error}")),
            }
        }
        Err(anyhow!(
            "failed to load MoltenVK from any known location: {}",
            attempts.join("; ")
        ))
    }
}
