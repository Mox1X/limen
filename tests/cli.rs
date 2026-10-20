use std::process::Command;
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_limen"))
}
#[test]
fn help_is_available_without_a_terminal() {
    let output = binary().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--cell-aspect"));
}
#[test]
fn invalid_sampling_fps_and_aspect_are_rejected() {
    for args in [
        ["--fps", "0"],
        ["--samples-x", "0"],
        ["--samples-y", "17"],
        ["--cell-aspect", "NaN"],
        ["--cell-aspect", "0"],
        ["--rotate-x", "NaN"],
        ["--rotate-y", "inf"],
        ["--rotate-z", "-inf"],
        ["--zoom", "0.4"],
        ["--zoom", "3.1"],
        ["--contrast", "0"],
        ["--gamma", "NaN"],
        ["--gamma", "4.1"],
        ["--ambient", "1.1"],
        ["--light-elevation", "91"],
        ["--light-azimuth", "NaN"],
        ["--view", "rgb"],
        ["--glyph-mode", "unicode"],
    ] {
        let output = binary().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(!output.stderr.contains(&0x1b));
    }
}
#[test]
fn missing_asset_fails_before_terminal_setup() {
    let output = binary().arg("/missing/limen-model.obj").output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("limen-model.obj"));
    assert!(!error.contains('\x1b'));
}
#[test]
fn redirected_output_has_a_clear_error() {
    let output = binary().output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal required"));
    assert!(output.stdout.is_empty());
}

#[test]
fn list_objects_works_without_a_terminal() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/enclosed.obj");
    let output = binary().arg(&path).arg("--list-objects").output().unwrap();
    assert!(output.status.success());
    let list = String::from_utf8_lossy(&output.stdout);
    assert!(list.contains("subject\t1 triangles"));
    assert!(list.contains("enclosure\t12 triangles"));
    assert!(!output.stdout.contains(&0x1b));
    let missing = binary()
        .arg(&path)
        .args(["--object", "missing"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("available objects: subject, enclosure")
    );
}

#[test]
fn object_flags_require_a_file_and_listing_conflicts_with_selection() {
    for args in [
        vec!["--list-objects"],
        vec!["--object", "subject"],
        vec!["model.obj", "--list-objects", "--object", "subject"],
    ] {
        assert!(!binary().args(args).output().unwrap().status.success());
    }
}

#[test]
fn finite_negative_initial_rotations_are_accepted() {
    let output = binary()
        .args([
            "--rotate-x",
            "-180",
            "--rotate-y",
            "30",
            "--rotate-z",
            "360",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal required"));
}

#[test]
fn display_options_accept_valid_modes_angles_and_bounds() {
    for mode in [
        "shaded",
        "depth",
        "silhouette",
        "normal-x",
        "normal-y",
        "normal-z",
    ] {
        let output = binary()
            .args([
                "--view",
                mode,
                "--light-azimuth",
                "-55",
                "--light-elevation",
                "-35",
                "--zoom",
                "3",
                "--contrast",
                "0.25",
                "--gamma",
                "4",
                "--ambient",
                "0",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal required"));
    }
}

#[test]
fn both_glyph_algorithms_accept_all_diagnostic_views() {
    for glyph in ["brightness", "shape"] {
        for view in [
            "shaded",
            "depth",
            "silhouette",
            "normal-x",
            "normal-y",
            "normal-z",
        ] {
            let output = binary()
                .args(["--glyph-mode", glyph, "--view", view])
                .output()
                .unwrap();
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("interactive terminal required")
            );
        }
    }
}
