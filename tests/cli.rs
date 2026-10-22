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

#[test]
fn font_options_validate_before_terminal_setup() {
    let font = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fonts/JetBrainsMono-Regular.ttf");
    for args in [vec!["--font-size", "16"], vec!["--font-cell", "10x22"]] {
        assert!(!binary().args(args).output().unwrap().status.success());
    }
    for (flag, value) in [
        ("--font-size", "NaN"),
        ("--font-size", "3"),
        ("--font-size", "129"),
        ("--font-cell", "0x10"),
        ("--font-cell", "513x10"),
        ("--font-cell", "10"),
        ("--font-cell", "10xNaN"),
    ] {
        let output = binary()
            .arg("--font")
            .arg(&font)
            .args([flag, value])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!output.stdout.contains(&0x1b) && !output.stderr.contains(&0x1b));
    }
    for glyph in ["brightness", "shape"] {
        let output = binary()
            .arg("--font")
            .arg(&font)
            .args([
                "--glyph-mode",
                glyph,
                "--font-size",
                "16",
                "--font-cell",
                "10x22",
            ])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal required"));
    }
    let output = binary()
        .args(["--font", "/missing/font.ttf"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("/missing/font.ttf"));
    assert!(!output.stderr.contains(&0x1b));
    let output = binary()
        .arg("--font")
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot parse font"));
    assert!(!output.stderr.contains(&0x1b));
}

#[test]
fn palettes_and_selection_controls_validate_before_terminal_setup() {
    for args in [
        vec!["--palette", "unknown"],
        vec!["--palette", "full"],
        vec!["--complexity-penalty", "NaN"],
        vec!["--complexity-penalty", "-0.1"],
        vec!["--stability", "inf"],
        vec!["--stability", "1.1"],
    ] {
        let output = binary().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(!output.stdout.contains(&0x1b) && !output.stderr.contains(&0x1b));
    }
    let output = binary().args(["--palette", "full"]).output().unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires --font"));
    for palette in ["tonal", "structural"] {
        let output = binary()
            .args([
                "--palette",
                palette,
                "--glyph-mode",
                "hybrid",
                "--complexity-penalty",
                "0",
                "--stability",
                "0",
            ])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal required"));
    }
}

#[test]
fn hand_composition_options_validate_before_terminal_setup() {
    for args in [
        vec!["--center-width", "0.42"],
        vec!["--hands-gap", "1"],
        vec!["--hands-brightness", "0.65"],
        vec!["--hands", "missing.png", "--center-width", "NaN"],
        vec!["--hands", "missing.png", "--center-width", "0.1"],
        vec!["--hands", "missing.png", "--hands-gap", "11"],
        vec!["--hands", "missing.png", "--hands-gap", "NaN"],
        vec!["--hands", "missing.png", "--hands-brightness", "1.1"],
    ] {
        let output = binary().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(!output.stdout.contains(&0x1b) && !output.stderr.contains(&0x1b));
    }
    let output = binary()
        .args(["--hands", "/missing/creation.png"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("/missing/creation.png"));
    assert!(!output.stdout.contains(&0x1b) && !output.stderr.contains(&0x1b));
    let output = binary()
        .arg("--hands")
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("interactive terminal required"));
}

#[test]
fn rotation_speed_and_status_flags_validate_without_a_terminal() {
    for value in ["NaN", "inf", "-inf", "-0.1", "360.1"] {
        let output = binary()
            .args(["--rotation-speed", value, "--hide-status"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("interactive terminal required"));
        assert!(!output.stdout.contains(&0x1b) && !output.stderr.contains(&0x1b));
    }
    for value in ["0", "0.25", "6", "360"] {
        let output = binary()
            .args(["--rotation-speed", value, "--hide-status"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal required"));
        assert!(output.stdout.is_empty());
    }
}
