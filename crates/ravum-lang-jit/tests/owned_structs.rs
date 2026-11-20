use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_ir::lower::LowerError;
use ravum_lang_ir::{compile_source, CompileError};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

#[test]
fn owned_fields_survive_moves_returns_and_mutable_borrows() {
    for value in [-17, 0, 1, 61] {
        for label in ["", "ravum", "été"] {
            let source = format!(
                r#"
struct Payload {{ label: String, values: Vec<i64> }}
struct Envelope {{ payload: Payload, tag: i64 }}

fn update(mut envelope: Envelope) -> Envelope effects(alloc) {{
    envelope.payload.label.push_str("!");
    envelope.payload.values.push(envelope.tag);
    return envelope;
}}

fn rename(envelope: &mut Envelope) -> () effects(alloc) {{
    envelope.payload.label.push_str("?");
}}

fn main() -> () effects(io, alloc) {{
    let payload = Payload {{ label: String::from("{label}"), values: Vec::new() }};
    let input = Envelope {{ payload: payload, tag: {value} }};
    let mut output = update(input);
    rename(&mut output);
    io::println(&output.payload.label);
    io::println(&output.payload.values[0].to_string());
}}
"#
            );
            let ir = compile_source(&source).expect("owned aggregate lowers");
            let (interpreted, interpreted_output) = run_with_output(&ir, "main", DEFAULT_STEPS);
            let (native, native_output) = compile(&ir)
                .expect("owned aggregate compiles")
                .run_with_output("main", DEFAULT_FUEL);
            assert_eq!(native.render(), "value () ()");
            assert_eq!(native.render(), interpreted.render());
            assert_eq!(native_output, interpreted_output);
            assert_eq!(native_output.stdout, format!("{label}!?\n{value}\n"));
        }
    }
}

#[test]
fn reusing_a_moved_struct_is_rejected_before_lowering() {
    let source = r#"
struct Payload { label: String }

fn main() -> () effects(io, alloc) {
    let payload = Payload { label: String::from("ravum") };
    let moved = payload;
    io::println(&moved.label);
    io::println(&payload.label);
}

"#;
    match compile_source(source) {
        Err(CompileError::Rejected(codes)) => {
            assert!(
                codes.iter().any(|code| code == "E-OWN-USE-AFTER-MOVE"),
                "{codes:?}"
            );
        }
        other => panic!("expected use-after-move refusal, got {other:?}"),
    }
}

#[test]
fn replacing_an_object_through_a_heap_borrow_is_refused() {
    let source = r#"
struct Payload { label: String }

fn replace(label: &mut String) -> () effects(alloc) {
    *label = String::from("new");
}

fn main() -> () effects(alloc) {
    let mut payload = Payload { label: String::from("old") };
    replace(&mut payload.label);
}
"#;
    match compile_source(source) {
        Err(CompileError::Lower(LowerError::Unsupported(detail))) => {
            assert_eq!(detail, "explicit dereference of a heap object borrow");
        }
        other => panic!("expected an explicit heap borrow refusal, got {other:?}"),
    }
}
