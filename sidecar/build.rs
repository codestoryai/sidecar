use std::{collections::HashMap, env, fs::{File, read_to_string}, io::{BufWriter, Write}, path::Path};
use serde::Deserialize;

#[derive(Deserialize)]
struct Language {
    r#type: String,
    aliases: Option<Vec<String>>,
}

#[cfg(feature = "grpc")]
fn compile_protos() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=proto/agent_farm.proto");
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .field_attribute(".", "#[allow(unused_variables)]")
        .compile(&["proto/agent_farm.proto"], &["proto"])
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to compile protos: {}", e),
        ))?;
    Ok(())
}

fn main() -> std::io::Result<()> {
    #[cfg(feature = "grpc")]
    compile_protos()?;

    let out_dir = env::var("OUT_DIR").map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to get OUT_DIR: {}", e))
    })?;

    // Generate version hash
    let mut hasher = blake3::Hasher::new();
    for path in ["languages.yml"].iter() {
        hasher.update(read_to_string(path)?.as_bytes());
        println!("cargo:rerun-if-changed={path}");
    }

    let version_file = Path::new(&out_dir).join("version_hash.rs");
    write!(
        BufWriter::new(File::create(version_file)?),
        r#"pub const BINARY_VERSION_HASH: &str = "{}";"#,
        hasher.finalize()
    )?;

    // Generate language maps
    let langs_file = File::open("./languages.yml")?;
    let langs: HashMap<String, Language> = serde_yaml::from_reader(langs_file)
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to parse languages.yml: {}", e)
        ))?;

    let languages_path = Path::new(&out_dir).join("languages.rs");
    let mut ext_map = phf_codegen::Map::new();
    let mut case_map = phf_codegen::Map::new();

    for (name, data) in langs.into_iter()
        .filter(|(_, d)| d.r#type == "programming" || d.r#type == "prose")
    {
        let name_lower = name.to_ascii_lowercase();
        
        for alias in data.aliases.unwrap_or_default() {
            ext_map.entry(alias, &format!("\"{name_lower}\""));
        }
        
        case_map.entry(name_lower, &format!("\"{name}\""));
    }

    write!(
        BufWriter::new(File::create(languages_path)?),
        "pub static EXT_MAP: phf::Map<&str, &str> = \n{};\n\
         pub static PROPER_CASE_MAP: phf::Map<&str, &str> = \n{};\n",
        ext_map.build(),
        case_map.build(),
    )?;

    println!("cargo:rerun-if-changed=./languages.yml");
    Ok(())
}