use std::{env, fs, path::PathBuf};

fn main() {
    let shaders = [
        ("shaders/vert.glsl", "vert.spv", shaderc::ShaderKind::Vertex),
        (
            "shaders/frag.glsl",
            "frag.spv",
            shaderc::ShaderKind::Fragment,
        ),
        (
            "shaders/frag_dedicated.glsl",
            "frag_dedicated.spv",
            shaderc::ShaderKind::Fragment,
        ),
    ];

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let compiler = shaderc::Compiler::new().expect("failed to create shaderc compiler");

    let mut options =
        shaderc::CompileOptions::new().expect("failed to create shaderc compile options");

    options.set_target_env(
        shaderc::TargetEnv::Vulkan,
        shaderc::EnvVersion::Vulkan1_0 as u32,
    );

    #[cfg(debug_assertions)]
    options.set_generate_debug_info();

    #[cfg(not(debug_assertions))]
    options.set_optimization_level(shaderc::OptimizationLevel::Performance);

    for (src_path, out_name, kind) in shaders {
        println!("cargo:rerun-if-changed={src_path}");

        let source = fs::read_to_string(src_path)
            .unwrap_or_else(|err| panic!("failed to read shader {src_path}: {err}"));

        let artifact = compiler
            .compile_into_spirv(&source, kind, src_path, "main", Some(&options))
            .unwrap_or_else(|err| panic!("failed to compile shader {src_path}:\n{err}"));

        let out_path = out_dir.join(out_name);

        fs::write(&out_path, artifact.as_binary_u8()).unwrap_or_else(|err| {
            panic!(
                "failed to write compiled shader {}: {err}",
                out_path.display()
            )
        });
    }
}
