use super::*;

#[test]
fn value_paths_reject_invalid_resolution_facts() {
    let mut builder = Builder::new();
    let path = builder.path(&["value"]);
    let mut program = build_program_e();
    let options = GenOptions::default();
    let cases = [
        (Some(Resolution::Module), "used as a value"),
        (Some(Resolution::BuiltinModule), "used as a value"),
        (Some(Resolution::Error), "is unresolved"),
        (
            Some(Resolution::BuiltinConst {
                path: "unknown::constant".to_string(),
            }),
            "unsupported builtin constant",
        ),
        (None, "missing resolution fact"),
    ];
    for (resolution, expected) in cases {
        if let Some(resolution) = resolution {
            program
                .checked
                .facts
                .resolutions
                .insert(path.id, resolution);
        } else {
            program.checked.facts.resolutions.remove(&path.id);
        }
        let emitter = Emitter {
            checked: &program.checked,
            options: &options,
            index: Index::build(&program.checked),
            array_moves: Default::default(),
        };
        let error = emitter
            .value_path(&path)
            .expect_err("invalid facts must not produce a Rust path");
        assert!(error.message.contains(expected), "{}", error.message);
    }
}
