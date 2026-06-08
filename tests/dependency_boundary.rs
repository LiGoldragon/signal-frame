use std::{path::PathBuf, process::Command};

struct Manifest {
    path: PathBuf,
}

impl Manifest {
    fn from_environment() -> Self {
        Self {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        }
    }

    fn cargo_tree(&self, arguments: &[&str]) -> String {
        let output = Command::new(env!("CARGO"))
            .arg("tree")
            .args(arguments)
            .current_dir(&self.path)
            .output()
            .expect("cargo tree runs");
        assert!(
            output.status.success(),
            "cargo tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("cargo tree output is UTF-8")
    }
}

#[test]
fn default_binary_kernel_does_not_depend_on_nota_next() {
    let manifest = Manifest::from_environment();
    let tree = manifest.cargo_tree(&["--edges", "normal", "--no-default-features"]);

    assert!(
        !tree.contains("nota-next") && !tree.contains("nota_next"),
        "default binary signal-frame tree must not contain nota-next:\n{tree}"
    );
}

#[test]
fn nota_text_feature_adds_nota_next() {
    let manifest = Manifest::from_environment();
    let tree = manifest.cargo_tree(&[
        "--edges",
        "normal",
        "--no-default-features",
        "--features",
        "nota-text",
    ]);

    assert!(
        tree.contains("nota-next"),
        "nota-text feature must add nota-next:\n{tree}"
    );
}
