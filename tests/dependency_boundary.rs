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
fn default_binary_kernel_does_not_depend_on_nota() {
    let manifest = Manifest::from_environment();
    let tree = manifest.cargo_tree(&["--edges", "normal", "--no-default-features"]);

    assert!(
        !tree.contains("nota") && !tree.contains("nota"),
        "default binary signal-frame tree must not contain nota:\n{tree}"
    );
}

#[test]
fn nota_text_feature_adds_nota() {
    let manifest = Manifest::from_environment();
    let tree = manifest.cargo_tree(&[
        "--edges",
        "normal",
        "--no-default-features",
        "--features",
        "nota-text",
    ]);

    assert!(
        tree.contains("nota"),
        "nota-text feature must add nota:\n{tree}"
    );
}

#[test]
fn generated_production_path_has_no_unbound_escape_hatch() {
    let emit = include_str!("../macros/src/emit.rs");
    let client = include_str!("../src/command_line.rs");

    for source in [emit, client] {
        assert!(
            !source.contains("legacy_unbound"),
            "generated production path must not create legacy-unbound headers"
        );
        assert!(
            !source.contains("::signal_frame::ExchangeFrame<")
                && !source.contains("::signal_frame::StreamingFrame<"),
            "generated production path must not use generic legacy frame aliases"
        );
    }
}
