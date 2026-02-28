mod c;
mod cpp;
mod java;
mod javascript;
mod python;
mod rust;

use c::C_DOCKERFILE;
use cpp::CPP_DOCKERFILE;
use java::JAVA_DOCKERFILE;
use javascript::JS_DOCKERFILE;
use python::PYTHON_DOCKERFILE;
use rust::RUST_DOCKERFILE;

pub fn general_dockerfile(repo_dir: &str, lang: &str) -> std::io::Result<()> {
    let content = match lang {
        "javascript" => JS_DOCKERFILE,
        "python" => PYTHON_DOCKERFILE,
        "rust" => RUST_DOCKERFILE,
        "java" => JAVA_DOCKERFILE,
        "c" => C_DOCKERFILE,
        "cpp" => CPP_DOCKERFILE,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Unsupported language",
            ))
        }
    };
    let path = format!("{}/Dockerfile", repo_dir);
    std::fs::write(path, content)
}
